package app.nukera.services

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.lifecycle.lifecycleScope
import app.nukera.R
import app.nukera.activities.MainActivity
import app.nukera.core.NukeraProxy
import app.nukera.core.NukeraProxyPreferences
import app.nukera.core.TProxyService
import app.nukera.core.HostsDnsServer
import app.nukera.data.*
import app.nukera.utility.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File

class NukeraVpnService : LifecycleVpnService() {
    private val byeDpiProxy = NukeraProxy()
    private var proxyJob: Job? = null
    private var tunFd: ParcelFileDescriptor? = null
    private val mutex = Mutex()
    private var mtuTick = 0

    private fun nextDnsIp(): String {
        val prefs = getPreferences()
        val count = prefs.getInt("dns_toggle_count", 0)
        prefs.edit().putInt("dns_toggle_count", count + 1).apply()
        return if (count % 2 == 0) "1.1.1.1" else "1.0.0.1"
    }
    private val dnsServer = HostsDnsServer()

    companion object {
        private val TAG: String = NukeraVpnService::class.java.simpleName
        private const val FOREGROUND_SERVICE_ID: Int = 1
        private const val PAUSE_NOTIFICATION_ID: Int = 3
        private const val NOTIFICATION_CHANNEL_ID: String = "NukeraVpn"

        private var status: ServiceStatus = ServiceStatus.Disconnected

        @JvmField
        var diagSelfAllowed: Boolean = false
    }

    override fun onCreate() {
        super.onCreate()
        registerNotificationChannel(
            this,
            NOTIFICATION_CHANNEL_ID,
            R.string.vpn_channel_name,
        )
    }

    override fun onDestroy() {
        super.onDestroy()
        tunFd?.close()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)

        startForeground()

        return when (val action = intent?.action) {
            START_ACTION -> {
                lifecycleScope.launch {
                    start()
                }
                START_STICKY
            }

            STOP_ACTION -> {
                lifecycleScope.launch {
                    stop()
                }
                START_NOT_STICKY
            }

            RESUME_ACTION -> {
                lifecycleScope.launch {
                    if (prepare(this@NukeraVpnService) == null) {
                        start()
                    }
                }
                START_STICKY
            }

            PAUSE_ACTION -> {
                lifecycleScope.launch {
                    stop()
                    createNotificationPause()
                }
                START_NOT_STICKY
            }

            SERVICE_INTERFACE -> {
                Log.i(TAG, "Started by Android")

                if (getPreferences().mode() != Mode.VPN) {
                    Log.w(TAG, "Always-On disabled in proxy mode")
                    stopSelf()
                    return START_NOT_STICKY
                }

                lifecycleScope.launch {
                    start()
                }

                START_STICKY
            }

            else -> {
                Log.w(TAG, "Unknown action: $action")
                START_NOT_STICKY
            }
        }
    }

    override fun onRevoke() {
        Log.i(TAG, "VPN revoked")
        lifecycleScope.launch { stop() }
    }

    private suspend fun start() {
        Log.i(TAG, "Starting")

        val notificationManager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        notificationManager.cancel(PAUSE_NOTIFICATION_ID)

        if (status == ServiceStatus.Connected) {
            Log.w(TAG, "VPN already connected")
            updateStatus(ServiceStatus.Connected)
            return
        }

        try {
            mutex.withLock {
                startProxy()
                startTun2Socks()
                updateStatus(ServiceStatus.Connected)
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start VPN", e)
            updateStatus(ServiceStatus.Failed)
            stop()
        }
    }

    private fun startForeground() {
        val notification: Notification = createNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                FOREGROUND_SERVICE_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED,
            )
        } else {
            startForeground(FOREGROUND_SERVICE_ID, notification)
        }
    }

    private suspend fun stop() {
        Log.i(TAG, "Stopping")

        if (status != ServiceStatus.Connected) {
            Log.w(TAG, "VPN not connected")
            updateStatus(ServiceStatus.Disconnected)
            return
        }

        mutex.withLock {
            try {
                withContext(Dispatchers.IO) {
                    stopProxy()
                    stopTun2Socks()
                }
            } catch (e: Exception) {
                Log.e(TAG, "Failed to stop VPN", e)
            }
        }

        updateStatus(ServiceStatus.Disconnected)
        stopSelf()
    }

    private fun startProxy() {
        Log.i(TAG, "Starting proxy")

        if (proxyJob != null) {
            Log.w(TAG, "Proxy fields not null")
            throw IllegalStateException("Proxy fields not null")
        }

        val preferences = getNukeraPreferences()

        proxyJob = lifecycleScope.launch(Dispatchers.IO) {
            var code = byeDpiProxy.startProxy(preferences)

            if (code != 0) {
                Log.w(TAG, "Proxy start failed ($code), clearing stale proxy state and retrying")
                try { byeDpiProxy.stopProxy() } catch (e: Exception) { Log.w(TAG, "stopProxy cleanup failed", e) }
                try { byeDpiProxy.jniForceClose() } catch (e: Exception) { Log.w(TAG, "forceClose cleanup failed", e) }
                delay(500)
                code = byeDpiProxy.startProxy(preferences)
            }

            delay(500)

            if (code != 0) {
                Log.e(TAG, "Proxy stopped with code $code")
                updateStatus(ServiceStatus.Failed)
                stopTun2Socks()
                stopSelf()
            }
        }

        Log.i(TAG, "Proxy started")
    }

    private suspend fun stopProxy() {
        Log.i(TAG, "Stopping proxy")

        if (status == ServiceStatus.Disconnected) {
            Log.w(TAG, "Proxy already disconnected")
            return
        }

        try {
            byeDpiProxy.stopProxy()
            proxyJob?.cancel()

            val completed = withTimeoutOrNull(2000) {
                proxyJob?.join()
                true
            }

            if (completed == null) {
                Log.w(TAG, "proxy not finish in time, cancelling...")
                byeDpiProxy.jniForceClose()
            }

            proxyJob = null
        } catch (e: Exception) {
            Log.e(TAG, "Failed to close proxyJob", e)
        }

        Log.i(TAG, "Proxy stopped")
    }

    private fun startTun2Socks() {
        Log.i(TAG, "Starting tun2socks")

        if (tunFd != null) {
            Log.w(TAG, "VPN field not null")
            throw IllegalStateException("VPN field not null")
        }

        val sharedPreferences = getPreferences()
        val (ip, port) = sharedPreferences.getProxyIpAndPort()

        val ipv6 = sharedPreferences.getBoolean("ipv6_enable", false)
        val dns = nextDnsIp()

        val tun2socksConfig = buildString {
            appendLine("tunnel:")
            appendLine("  mtu: 8500")

            appendLine("misc:")
            appendLine("  task-stack-size: 81920")
            appendLine("  log-level: debug")
            appendLine("  log-file: ${cacheDir.absolutePath}/hev.log")

            appendLine("socks5:")
            appendLine("  address: $ip")
            appendLine("  port: $port")
            appendLine("  udp: udp")

            val hostsCfg = loadHostsConfig()
            if (hostsCfg.isNotEmpty()) appendLine(hostsCfg)

            val redirectsCfg = loadRedirectsConfig()
            if (redirectsCfg.isNotEmpty()) appendLine(redirectsCfg)
        }

        val configPath = try {
            rotateHevLog()
            File.createTempFile("config", "tmp", cacheDir).apply {
                writeText(tun2socksConfig)
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to create config file", e)
            throw e
        }

        val fd = createBuilder(dns, ipv6).establish()
            ?: throw IllegalStateException("VPN connection failed")

        this.tunFd = fd

        TProxyService.TProxyStartService(configPath.absolutePath, fd.fd)

        Log.i(TAG, "Tun2Socks started. ip: $ip port: $port")
        prewarmTelegramDns()
    }

    private fun rotateHevLog() {
        try {
            val log = File(cacheDir, "hev.log")
            if (log.length() > 1024 * 1024) log.delete()
        } catch (_: Exception) {}
    }

    private fun stopTun2Socks() {
        Log.i(TAG, "Stopping tun2socks")

        dnsServer.stop()

        if (tunFd == null) {
            Log.w(TAG, "VPN field is null, skipping")
            return
        }

        try {
            TProxyService.TProxyStopService()
        } catch (e: Exception) {
            Log.e(TAG, "Failed to stop TProxyService", e)
        }

        try {
            File(cacheDir, "config.tmp").delete()
        } catch (e: SecurityException) {
            Log.e(TAG, "Failed to delete config file", e)
        }

        try {
            tunFd?.close()
        } catch (e: Exception) {
            Log.e(TAG, "Failed to close tunFd", e)
        } finally {
            tunFd = null
        }

        Log.i(TAG, "Tun2socks stopped")
    }

    private fun getNukeraPreferences(): NukeraProxyPreferences =
        NukeraProxyPreferences.fromSharedPreferences(getPreferences(), this)

    private fun loadHostsConfig(): String {
        return try {
            val hosts = mutableMapOf<String, MutableList<String>>()
            assets.open("telegram-web.txt").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val t = line.trim()
                    if (t.isEmpty() || t.startsWith("#")) return@forEach
                    val p = t.split("\\s+".toRegex(), limit = 2)
                    if (p.size == 2) hosts.getOrPut(p[0]) { mutableListOf() }.add(p[1])
                }
            }
            if (hosts.isEmpty()) return ""
            buildString {
                appendLine("hosts:")
                for ((ip, doms) in hosts) appendLine("  \"$ip\": \"${doms.joinToString(" ")}\"")
            }
        } catch (e: Exception) { Log.e(TAG, "hosts load fail", e); "" }
    }

    private fun loadRedirectsConfig(): String {
        return try {
            val targets = listOf(
                "149.154.167.220",
                "149.154.167.99",
                "149.154.167.198",
                "149.154.175.209",
                "149.154.166.110",
            )
            val srcs = mutableListOf<String>()
            assets.open("telegram-web-redirect.txt").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val t = line.trim()
                    if (t.isEmpty() || t.startsWith("#")) return@forEach
                    if (targets.contains(t)) return@forEach
                    srcs.add(t)
                }
            }
            if (srcs.isEmpty()) return ""
            buildString {
                appendLine("redirects:")
                appendLine("  \"${targets[0]}\": \"${srcs.joinToString(" ")}\"")
                for (i in 1 until targets.size) appendLine("  \"${targets[i]}\": \"\"")
            }
        } catch (e: Exception) { Log.e(TAG, "redirects load fail", e); "" }
    }

    private fun prewarmTelegramDns() {
        Thread {
            try {
                val hosts = mutableListOf<String>()
                assets.open("telegram-web.txt").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val t = line.trim()
                        if (t.isEmpty() || t.startsWith("#")) return@forEach
                        hosts.addAll(t.split("\\s+".toRegex(), limit = 2)[1].split(" "))
                    }
                }
                hosts.distinct().take(32).forEach { h ->
                    try { java.net.InetAddress.getByName(h) } catch (_: Exception) {}
                }
                Log.i(TAG, "Telegram DNS prewarm done (${hosts.size} hosts)")
            } catch (e: Exception) { Log.e(TAG, "prewarm fail", e) }
        }.start()
    }

    private fun updateStatus(newStatus: ServiceStatus) {
        Log.d(TAG, "VPN status changed from $status to $newStatus")

        status = newStatus

        setStatus(
            when (newStatus) {
                ServiceStatus.Connected -> AppStatus.Running

                ServiceStatus.Disconnected,
                ServiceStatus.Failed -> {
                    proxyJob = null
                    AppStatus.Halted
                }
            },
            Mode.VPN
        )

        val intent = Intent(
            when (newStatus) {
                ServiceStatus.Connected -> STARTED_BROADCAST
                ServiceStatus.Disconnected -> STOPPED_BROADCAST
                ServiceStatus.Failed -> FAILED_BROADCAST
            }
        )
        intent.putExtra(SENDER, Sender.VPN.ordinal)
        sendBroadcast(intent)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            QuickTileService.updateTile()
        }
    }

    private fun createNotification(): Notification =
        createConnectionNotification(
            this,
            NOTIFICATION_CHANNEL_ID,
            R.string.notification_title,
            R.string.vpn_notification_content,
            NukeraVpnService::class.java,
        )

    private fun createNotificationPause() {
        val notification = createPauseNotification(
            this,
            NOTIFICATION_CHANNEL_ID,
            R.string.notification_title,
            R.string.service_paused_text,
            NukeraVpnService::class.java,
        )

        val notificationManager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        notificationManager.notify(PAUSE_NOTIFICATION_ID, notification)
    }

    private fun createBuilder(dns: String, ipv6: Boolean): Builder {
        Log.d(TAG, "DNS: $dns")
        val builder = Builder()
        builder.setSession(if (dns == "1.1.1.1") "Nukera" else "Nukera2")
        builder.setConfigureIntent(
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_IMMUTABLE,
            )
        )

        // Guide Step 1: virtual DNS IP inside VPN subnet
        builder.addAddress("10.10.10.1", 24)
            .addRoute("0.0.0.0", 0)

        if (ipv6) {
            builder.addAddress("fd00::1", 128)
                .addRoute("::", 0)
        }

        if (dns.isNotBlank()) {
            builder.addDnsServer(dns)
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        builder.setMtu(if (mtuTick++ % 2 == 0) 1500 else 1280)

        val preferences = getPreferences()
        val listType = preferences.getStringNotNull("applist_type", "disable")
        val listedApps = preferences.getSelectedApps()

        when (listType) {
            "blacklist" -> {
                for (packageName in listedApps) {
                    try {
                        builder.addDisallowedApplication(packageName)
                    } catch (e: Exception) {
                        Log.e(TAG, "Не удалось добавить приложение $packageName в черный список", e)
                    }
                }

                builder.addDisallowedApplication(applicationContext.packageName)
            }

            "whitelist" -> {
                for (packageName in listedApps) {
                    try {
                        builder.addAllowedApplication(packageName)
                    } catch (e: Exception) {
                        Log.e(TAG, "Не удалось добавить приложение $packageName в белый список", e)
                    }
                }
            }

            "disable" -> {
                if (!diagSelfAllowed) builder.addDisallowedApplication(applicationContext.packageName)
            }
        }

        val telegramPackages = listOf(
            "org.telegram.messenger",
            "org.telegram.messenger.web",
            "org.telegram.plus",
            "org.telegram.messenger.alpha",
            "org.telegram.messenger.beta"
        )
        for (pkg in telegramPackages) {
            try {
                builder.addDisallowedApplication(pkg)
            } catch (_: Exception) {}
        }

        return builder
    }
}
