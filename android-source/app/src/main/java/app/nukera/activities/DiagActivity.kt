package app.nukera.activities

import android.graphics.Typeface
import android.os.Bundle
import android.os.SystemClock
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import app.nukera.data.Mode
import app.nukera.services.NukeraVpnService
import app.nukera.services.ServiceManager
import app.nukera.services.appStatus
import app.nukera.utility.getPreferences
import app.nukera.utility.getStringNotNull
import java.io.BufferedReader
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketTimeoutException
import java.net.URL
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

class DiagActivity : AppCompatActivity() {

    private val out = StringBuilder()
    private lateinit var report: TextView

    companion object {
        private const val TAG = "DiagActivity"
        private const val HOSTS_IP = "149.154.167.220"
        private val TEST_DOMAINS = listOf("web.telegram.org", "t.me", "api.telegram.org", "google.com")
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val vpnBtn = Button(this).apply { text = "Start VPN" }
        val diagBtn = Button(this).apply { text = "Run diagnostics" }
        report = TextView(this).apply {
            setTypeface(Typeface.MONOSPACE)
            textSize = 12f
        }
        val scroll = ScrollView(this).apply {
            addView(report, LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
        }
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(vpnBtn, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT))
            addView(diagBtn, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT))
            addView(scroll, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        })

        vpnBtn.setOnClickListener {
            NukeraVpnService.diagSelfAllowed = true
            thread {
                rerun { ServiceManager.start(this@DiagActivity, Mode.VPN) }
            }
        }
        diagBtn.setOnClickListener {
            thread {
                runAll()
                writeReport()
            }
        }

        if (intent.getBooleanExtra("autostart", false)) {
            NukeraVpnService.diagSelfAllowed = true
            thread {
                try {
                    if (appStatus.first == app.nukera.data.AppStatus.Running) {
                        ServiceManager.stop(this@DiagActivity)
                        waitHalted()
                    }
                } catch (_: Exception) {}
                rerun {
                    ServiceManager.start(this@DiagActivity, Mode.VPN)
                    val t0 = SystemClock.elapsedRealtime()
                    while (SystemClock.elapsedRealtime() - t0 < 15000) {
                        if (appStatus.first == app.nukera.data.AppStatus.Running) break
                        Thread.sleep(500)
                    }
                }
                runAll()
                writeReport()
            }
        }
    }

    private fun waitHalted() {
        val t0 = SystemClock.elapsedRealtime()
        while (SystemClock.elapsedRealtime() - t0 < 10000) {
            if (appStatus.first == app.nukera.data.AppStatus.Halted) return
            Thread.sleep(300)
        }
    }

    private inline fun rerun(block: () -> Unit) {
        try {
            block()
        } catch (e: Exception) {
            line("RUN FAIL ${e}")
        }
    }

    private fun line(s: String) {
        out.appendLine(s)
        runOnUiThread { report.text = report.text.toString() + s + "\n" }
    }

    private fun runAll() {
        out.setLength(0)
        runOnUiThread { report.text = "" }

        val prefs = getPreferences()
        val dns = prefs.getStringNotNull("dns_ip", "1.1.1.1")
        val mode = appStatus.second
        line("=== Nukera TG Web diagnostics ===")
        line("Build: ${packageManager.getPackageInfo(packageName, 0).versionName} (${
            String.format(java.util.Locale.US, "api %d", android.os.Build.VERSION.SDK_INT)})")
        line("Mode: $mode  app status: ${appStatus.first}")
        line("Configured DNS (vpn dns_ip): $dns")
        line("Hosts target IP: $HOSTS_IP")
        line("")

        line("--- 1. System resolver (netd), does it reach tunnel? ---")
        TEST_DOMAINS.forEach { d ->
            val ips = systemResolve(d)
            line("$d -> ${ips.joinToString(", ")}")
        }
        line("")

        line("--- 2. Raw UDP:53 to configured DNS (through tunnel) ---")
        rawQuery(dns, "web.telegram.org")
        rawQuery(dns, "t.me")
        rawQuery(dns, "google.com")
        line("")

        line("--- 3. Raw UDP:53 to 8.8.8.8 (dest-independence of intercept) ---")
        rawQuery("8.8.8.8", "web.telegram.org")
        rawQuery("8.8.8.8", "kws2.web.telegram.org")
        line("")

        line("--- 4. TCP/TLS reachability ---")
        tcpProbe(HOSTS_IP)
        line("")

        line("--- 5. HTTPS web.telegram.org (through VPN) ---")
        httpProbe("https://web.telegram.org/a")
        httpProbe("https://web.telegram.org/k/")
        line("")

        line("--- 6. tunnel C logs (last matching) ---")
        tailLogcat()
        line("")
        line("=== END ===")
    }

    private fun systemResolve(domain: String): List<String> {
        val outList = mutableListOf<String>()
        try {
            runCatching {
                val a = InetAddress.getAllByName(domain)
                outList.addAll(a.map { it.hostAddress })
            }.onFailure { e -> outList.add("ERR ${e::class.java.simpleName}:${e.message}") }
        } catch (e: Exception) {
            outList.add("EXC ${e.message}")
        }
        return outList
    }

    private fun rawQuery(dnsServer: String, domain: String) {
        val t0 = SystemClock.elapsedRealtime()
        try {
val sock = DatagramSocket(null)
            sock.soTimeout = 4000
            if (dnsServer == "1.1.1.1" && domain == "web.telegram.org") {
                sock.reuseAddress = true
                sock.bind(java.net.InetSocketAddress(java.net.InetAddress.getByName("0.0.0.0"), 15553))
                line("PROBE fixed local port 15553")
            }
            val q = buildAQuery(domain)
            sock.send(DatagramPacket(q, q.size, InetAddress.getByName(dnsServer), 53))
            val buf = ByteArray(1200)
            val p = DatagramPacket(buf, buf.size)
            val from = try {
                sock.receive(p)
                p.address.hostAddress
            } catch (e: SocketTimeoutException) {
                "TIMEOUT(${(SystemClock.elapsedRealtime() - t0)}ms)"
            }
            val rtt = SystemClock.elapsedRealtime() - t0
            val ips = parseAnswers(p.data, p.length)
            line("$domain @ $dnsServer:53 <- $from  ${rtt}ms  answers=$ips  rxlen=${p.length}")
            line("  RX ${p.data.copyOf(p.length).joinToString("") { "%02x".format(it) }}")
            sock.close()
        } catch (e: Exception) {
            line("$domain @ $dnsServer:53  EXC ${e::class.java.simpleName}:${e.message}")
        }
    }

    private fun tcpProbe(ip: String) {
        val t0 = SystemClock.elapsedRealtime()
        try {
            Socket().use { s ->
                s.soTimeout = 4000
                s.connect(InetSocketAddress(ip, 443), 4000)
                line("TCP $ip:443 connected in ${SystemClock.elapsedRealtime() - t0}ms")
            }
        } catch (e: Exception) {
            line("TCP $ip:443 FAIL ${e::class.java.simpleName}:${e.message}")
        }
    }

    private fun httpProbe(urlStr: String) {
        val t0 = SystemClock.elapsedRealtime()
        try {
            val conn = (URL(urlStr).openConnection() as HttpURLConnection).apply {
                connectTimeout = 6000
                readTimeout = 6000
                instanceFollowRedirects = true
                setRequestProperty("User-Agent", "Mozilla/5.0 (X11; Linux x86_64)")
            }
            val code = conn.responseCode
            val ip = URL(urlStr).host
            val body = try {
                conn.inputStream.bufferedReader().use { it.readText().take(4096).length }
            } catch (_: Exception) { -1 }
            line("$urlStr -> HTTP $code  ip=$ip  readBytes=$body  ${SystemClock.elapsedRealtime() - t0}ms")
            conn.disconnect()
        } catch (e: Exception) {
            line("$urlStr FAIL ${e::class.java.simpleName}:${e.message}  ${SystemClock.elapsedRealtime() - t0}ms")
        }
    }

    private fun tailLogcat() {
        try {
            val logs = StringBuilder()
            try {
                val f = File(cacheDir, "hev.log")
                if (f.exists()) {
                    val lines = f.readText().lines()
                    val want = lines.filter {
                        it.contains("dns") || it.contains("hosts") || it.contains("intercept") ||
                            it.contains("udp") || it.contains("lwip") || it.contains("tunnel") ||
                            it.contains("config") || it.contains("mapped")
                    }
                    logs.append("hev.log (${f.length()} bytes):\n")
                    want.takeLast(60).forEach { logs.append("  ").append(it).append('\n') }
                } else logs.append("hev.log missing\n")
            } catch (e: Exception) {
                logs.append("hev.log FAIL ${e.message}\n")
            }
            logs.append("logcat:\n")
            val proc = Runtime.getRuntime().exec(arrayOf("logcat", "-d", "-t", "800"))
            val outBytes = ByteArrayOutputStream()
            proc.inputStream.copyTo(outBytes)
            proc.waitFor()
            val lines = String(outBytes.toByteArray()).lines()
            val matching = lines.filter {
                it.contains("dns hosts") || it.contains("dns intercept") ||
                    it.contains("drop dns over tls") || it.contains("socks5 tunnel") ||
                    it.contains("TProxyService") || it.contains("NukeraVpnService")
            }
            matching.takeLast(40).forEach { logs.append("  ").append(it).append('\n') }
            if (logs.isBlank()) logs.append("  (no matching lines)")
            logs.toString().lines().takeLast(120).forEach { line(it) }
        } catch (e: Exception) {
            line("  logcat FAIL ${e.message}")
        }
    }

    private fun buildAQuery(domain: String): ByteArray {
        val baos = ByteArrayOutputStream()
        baos.write(0x12); baos.write(0x34)
        baos.write(0x01); baos.write(0x00)
        baos.write(0x00); baos.write(0x01)
        baos.write(0); baos.write(0); baos.write(0); baos.write(0); baos.write(0); baos.write(0)
        domain.split(".").forEach { label ->
            baos.write(label.length)
            baos.write(label.toByteArray())
        }
        baos.write(0)
        baos.write(0); baos.write(1)
        baos.write(0); baos.write(1)
        return baos.toByteArray()
    }

    private fun parseAnswers(data: ByteArray, len: Int): List<String> {
        if (len < 12) return listOf("SHORT($len)")
        val fl = ((data[2].toInt() and 0xFF shl 8) or (data[3].toInt() and 0xFF))
        val qd = ((data[4].toInt() and 0xFF shl 8) or (data[5].toInt() and 0xFF))
        val an = ((data[6].toInt() and 0xFF shl 8) or (data[7].toInt() and 0xFF))
        val rcode = fl and 0xF
        var pos = 12
        for (i in 0 until qd) {
            while (pos < len && data[pos].toInt() != 0) pos += 1 + (data[pos].toInt() and 0xFF)
            pos += 5
            if (pos >= len) return listOf("PARSE_FAIL qd")
        }
        val ips = mutableListOf<String>()
        for (i in 0 until an) {
            if (pos + 10 > len) break
            pos = skipName(data, pos, len)
            if (pos + 10 > len) break
            val type = (data[pos].toInt() and 0xFF shl 8) or (data[pos + 1].toInt() and 0xFF)
            val rdLen = ((data[pos + 8].toInt() and 0xFF shl 8) or (data[pos + 9].toInt() and 0xFF))
            pos += 10
            if (type == 1 && rdLen == 4 && pos + 4 <= len) {
                ips.add((data[pos].toInt() and 0xFF).toString() + "." +
                    (data[pos + 1].toInt() and 0xFF) + "." +
                    (data[pos + 2].toInt() and 0xFF) + "." +
                    (data[pos + 3].toInt() and 0xFF))
            }
            pos += rdLen
        }
        if (fl and 0x8000 == 0) return listOf("QR=0 (malformed)")
        if (rcode != 0) return listOf("RCODE=$rcode")
        return if (ips.isEmpty()) listOf("(none)") else ips
    }

private fun skipName(data: ByteArray, start: Int, len: Int): Int {
        var pos = start
        while (pos < len) {
            val b = data[pos].toInt() and 0xFF
            if (b == 0) { pos++; break }
            if (b and 0xC0 == 0xC0) { pos += 2; break }
            else pos += 1 + b
        }
        return pos
    }

    private fun writeReport() {
        try {
            val f = File(filesDir, "diag_report.txt")
            f.writeText(out.toString())
            line("")
            line("report: $f")
        } catch (e: Exception) {
            line("writeReport FAIL ${e.message}")
        }
    }
}