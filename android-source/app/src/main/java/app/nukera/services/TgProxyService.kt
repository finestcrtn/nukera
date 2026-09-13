package app.nukera.services

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import app.nukera.R
import app.nukera.activities.MainActivity
import app.nukera.core.NativeProxy
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.security.SecureRandom

class TgProxyService : Service() {

    private var wakeLock: PowerManager.WakeLock? = null
    private val serviceScope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private var secretKey: String = ""

    companion object {
        private const val TAG = "TgProxyService"
        private const val CHANNEL_ID = "TgProxyChannel"
        private const val NOTIFICATION_ID = 200
        private const val TG_PORT = 1444
        private const val PREFS_NAME = "tg_proxy_prefs"
        private const val KEY_SECRET = "secret_key"
        private const val WAKELOCK_TIMEOUT_MS = 30L * 60 * 1000L

        const val ACTION_START = "app.nukera.TG_START"
        const val ACTION_STOP = "app.nukera.TG_STOP"

        private val _isRunning = MutableStateFlow(false)
        val isRunning: StateFlow<Boolean> = _isRunning

        fun getProxyUrl(context: Context): String {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            val secret = prefs.getString(KEY_SECRET, "") ?: ""
            if (secret.isEmpty()) return ""
            return "tg://proxy?server=127.0.0.1&port=$TG_PORT&secret=dd$secret"
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startProxy()
            ACTION_STOP -> stopProxy()
        }
        return START_NOT_STICKY
    }

    private fun startProxy() {
        if (_isRunning.value) return

        secretKey = loadOrCreateSecret()

        val notification = createNotification("Starting TG proxy...")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        acquireWakeLock()

        serviceScope.launch {
            val result = withTimeoutOrNull(5000L) {
                CompletableDeferred<Int>().apply {
                    Thread({
                        try {
                            NativeProxy.setPoolSize(4)
                            NativeProxy.setCfProxyCacheDir(cacheDir.absolutePath)
                            NativeProxy.setCfProxyConfig(true, true, "")
                            val code = NativeProxy.startProxy("127.0.0.1", TG_PORT, "", secretKey, 0)
                            complete(code)
                        } catch (e: Exception) {
                            Log.e(TAG, "StartProxy failed", e)
                            complete(-1)
                        }
                    }, "TgProxyStart").apply {
                        isDaemon = true
                        start()
                    }
                }.await()
            }

            if (result == 0) {
                _isRunning.value = true
                updateNotification("TG proxy running")
                Log.i(TAG, "TG proxy started on port $TG_PORT")
            } else {
                Log.e(TAG, "TG proxy failed to start, code=$result")
                stopProxy()
            }
        }
    }

    private fun stopProxy() {
        if (!_isRunning.value && wakeLock == null) {
            stopSelf()
            return
        }

        serviceScope.launch {
            updateNotification("Stopping...")
            val completed = withTimeoutOrNull(3000L) {
                CompletableDeferred<Unit>().apply {
                    Thread({
                        try {
                            NativeProxy.stopProxy()
                        } catch (e: Exception) {
                            Log.w(TAG, "StopProxy failed", e)
                        } finally {
                            complete(Unit)
                        }
                    }, "TgProxyStop").apply {
                        isDaemon = true
                        start()
                    }
                }.await()
            }

            if (completed == null) {
                Log.w(TAG, "Native stop timed out")
            }

            _isRunning.value = false
            releaseWakeLock()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            Log.i(TAG, "TG proxy stopped")
        }
    }

    private fun loadOrCreateSecret(): String {
        val prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        var secret = prefs.getString(KEY_SECRET, "") ?: ""
        if (secret.length == 32 && secret.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) {
            return secret
        }
        val bytes = ByteArray(16)
        SecureRandom().nextBytes(bytes)
        secret = bytes.joinToString("") { "%02x".format(it) }
        prefs.edit().putString(KEY_SECRET, secret).apply()
        return secret
    }

    private fun acquireWakeLock() {
        try {
            val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
            wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "TgProxy::WakeLock").apply {
                acquire(WAKELOCK_TIMEOUT_MS)
            }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to acquire WakeLock", e)
        }
    }

    private fun releaseWakeLock() {
        try {
            wakeLock?.let { if (it.isHeld) it.release() }
        } catch (_: Exception) {}
        wakeLock = null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(CHANNEL_ID, "TG Proxy", NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
                setSound(null, null)
                enableVibration(false)
            }
            getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
        }
    }

    private fun createNotification(text: String): Notification {
        val openIntent = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val stopIntent = PendingIntent.getService(
            this, 0,
            Intent(this, TgProxyService::class.java).apply { action = ACTION_STOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Telegram WS Proxy")
            .setContentText(text)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(openIntent)
            .addAction(Notification.Action.Builder(null, "Stop", stopIntent).build())
            .setOngoing(true)
            .build()
    }

    private fun updateNotification(text: String) {
        try {
            getSystemService(NotificationManager::class.java)
                ?.notify(NOTIFICATION_ID, createNotification(text))
        } catch (e: Exception) {
            Log.w(TAG, "Failed to update notification", e)
        }
    }

    override fun onDestroy() {
        serviceScope.cancel()
        releaseWakeLock()
        if (_isRunning.value) {
            _isRunning.value = false
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
