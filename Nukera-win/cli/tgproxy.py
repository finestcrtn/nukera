import asyncio
import os
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VENDOR_DIR = os.path.join(ROOT, "engine", "telegram-ws-proxy")
if VENDOR_DIR not in sys.path:
    sys.path.insert(0, VENDOR_DIR)

from tg_ws_proxy_vendor.proxy import tg_ws_proxy as flowseal_proxy  # noqa: E402
from tg_ws_proxy_vendor.proxy.config import proxy_config  # noqa: E402
from tg_ws_proxy_vendor.proxy.stats import stats as flowseal_stats  # noqa: E402

TGPROXY_HOST = "127.0.0.1"
TGPROXY_PORT = 1443


class TelegramProxyError(RuntimeError):
    pass


class TelegramProxyController:
    """Local MTProto (WebSocket-bridge) proxy for the Telegram Desktop app,
    ported from Zapret-GUI's telegram_proxy.py. Runs as a daemon thread inside
    whatever process hosts it — in Nukera that is the elevated worker, so the
    proxy lives as long as the worker does.
    """

    def __init__(self):
        self._thread = None
        self._loop = None
        self._stop_event = None
        self._started = threading.Event()
        self._stopped = threading.Event()
        self._lock = threading.RLock()
        self._host = TGPROXY_HOST
        self._port = TGPROXY_PORT
        self._secret = ""
        self._started_at = 0.0
        self._last_error = ""

    def start(self, port=TGPROXY_PORT, secret=None):
        port = int(port or TGPROXY_PORT)
        secret = self._normalize_secret(secret)

        with self._lock:
            if self.is_running():
                if port != self._port:
                    raise TelegramProxyError(
                        "Telegram proxy already runs on port %s" % self._port)
                if secret != self._secret:
                    raise TelegramProxyError(
                        "Telegram proxy already runs with another secret")
                return

            self._port = port
            self._secret = secret
            self._last_error = ""
            self._started_at = 0.0
            self._started.clear()
            self._stopped.clear()
            self._thread = threading.Thread(
                target=self._thread_main,
                name="Nukera-TelegramMTProtoProxy",
                daemon=True,
            )
            self._thread.start()

        if not self._started.wait(8.0):
            self.stop()
            raise TelegramProxyError("Telegram MTProto proxy start timed out")

        err = self._last_error
        if err and not self.is_running():
            raise TelegramProxyError(err)

    def stop(self):
        loop = self._loop
        stop_event = self._stop_event
        if loop is not None and loop.is_running() and stop_event is not None:
            try:
                loop.call_soon_threadsafe(stop_event.set)
            except Exception:
                pass

        thread = self._thread
        if thread is not None and thread.is_alive():
            thread.join(timeout=6.0)

        with self._lock:
            self._thread = None
            self._loop = None
            self._stop_event = None

    def is_running(self):
        return bool(
            self._thread is not None
            and self._thread.is_alive()
            and self._started.is_set()
            and not self._stopped.is_set()
        )

    def proxy_link(self):
        return "tg://proxy?server=%s&port=%d&secret=dd%s" % (
            self._host, int(self._port), self._secret)

    def port(self):
        return int(self._port)

    def secret(self):
        return str(self._secret)

    def last_error(self):
        return str(self._last_error or "")

    def _thread_main(self):
        loop = asyncio.new_event_loop()
        self._loop = loop
        asyncio.set_event_loop(loop)
        self._stop_event = asyncio.Event()

        try:
            self._configure_flowseal_proxy()
            loop.run_until_complete(self._run_flowseal_proxy())
        except Exception as e:
            self._last_error = str(e)
            self._started.set()
        finally:
            self._stopped.set()
            try:
                loop.close()
            except Exception:
                pass

    async def _run_flowseal_proxy(self):
        task = asyncio.create_task(flowseal_proxy._run(self._stop_event))
        try:
            for _ in range(160):
                if flowseal_proxy._server_instance is not None:
                    self._started_at = time.time()
                    self._started.set()
                    break
                if task.done():
                    break
                await asyncio.sleep(0.05)

            if not self._started.is_set():
                self._started.set()

            await task
        finally:
            if not task.done():
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass

    def _configure_flowseal_proxy(self):
        proxy_config.host = self._host
        proxy_config.port = int(self._port)
        proxy_config.secret = self._secret
        proxy_config.dc_redirects = {2: "149.154.167.220", 4: "149.154.167.220"}
        proxy_config.buffer_size = 256 * 1024
        proxy_config.pool_size = 4
        proxy_config.fallback_cfproxy = True
        proxy_config.fallback_cfproxy_priority = True
        proxy_config.cfproxy_user_domain = ""
        proxy_config.fake_tls_domain = ""
        proxy_config.proxy_protocol = False

    @staticmethod
    def _normalize_secret(secret):
        value = str(secret or "").strip().lower()
        if not value:
            return os.urandom(16).hex()
        if value.startswith("dd") and len(value) == 34:
            value = value[2:]
        if len(value) != 32:
            raise TelegramProxyError(
                "Telegram MTProto secret must be 32 hex chars")
        try:
            bytes.fromhex(value)
        except ValueError as e:
            raise TelegramProxyError(
                "Telegram MTProto secret must be valid hex") from e
        return value


def load_or_create_secret(path):
    try:
        value = open(path, "r", encoding="utf-8").read().strip()
        if TelegramProxyController._normalize_secret(value) == value:
            return value
    except (OSError, TelegramProxyError):
        pass
    secret = os.urandom(16).hex()
    try:
        with open(path, "w", encoding="utf-8") as f:
            f.write(secret)
    except OSError:
        pass
    return secret


def make_proxy_link(port=TGPROXY_PORT, secret=""):
    return "tg://proxy?server=%s&port=%d&secret=dd%s" % (
        TGPROXY_HOST, int(port), secret)


def serve(port=TGPROXY_PORT, secret="", ready_file=None):
    """Standalone proxy server process entry point. The vendored proxy uses
    module-global state, so a fresh process per proxy run is cleaner than
    restarting a thread in a long-lived worker. Writes its PID to ready_file
    once the listener is up."""
    controller = TelegramProxyController()
    try:
        controller.start(port, secret)
    except TelegramProxyError as e:
        sys.stderr.write("FATAL Telegram proxy: %s\n" % str(e))
        sys.exit(1)
    try:
        if ready_file:
            with open(ready_file, "w", encoding="utf-8") as f:
                f.write("%s\n" % os.getpid())
        print("READY %s:%s" % (TGPROXY_HOST, int(port)), flush=True)
        while True:
            time.sleep(1.0)
    except KeyboardInterrupt:
        pass
    finally:
        controller.stop()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else TGPROXY_PORT
    secret = sys.argv[2] if len(sys.argv) > 2 else ""
    ready = sys.argv[3] if len(sys.argv) > 3 else ""
    serve(port, secret, ready)