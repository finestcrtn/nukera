package app.nukera.services

import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.content.ContextCompat
import app.nukera.data.AppStatus
import app.nukera.data.Mode
import app.nukera.data.START_ACTION
import app.nukera.data.STOP_ACTION
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

object ServiceManager {
    private val TAG: String = ServiceManager::class.java.simpleName
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    fun start(context: Context, mode: Mode) {
        when (mode) {
            Mode.VPN -> {
                Log.i(TAG, "Starting VPN")
                val intent = Intent(context, NukeraVpnService::class.java)
                intent.action = START_ACTION
                ContextCompat.startForegroundService(context, intent)
                startTgProxy(context)
            }

            Mode.Proxy -> {
                Log.i(TAG, "Starting proxy")
                val intent = Intent(context, NukeraProxyService::class.java)
                intent.action = START_ACTION
                ContextCompat.startForegroundService(context, intent)
            }
        }
    }

    fun stop(context: Context) {
        val (_, mode) = appStatus
        when (mode) {
            Mode.VPN -> {
                Log.i(TAG, "Stopping VPN")
                val intent = Intent(context, NukeraVpnService::class.java)
                intent.action = STOP_ACTION
                ContextCompat.startForegroundService(context, intent)
                stopTgProxy(context)
            }

            Mode.Proxy -> {
                Log.i(TAG, "Stopping proxy")
                val intent = Intent(context, NukeraProxyService::class.java)
                intent.action = STOP_ACTION
                ContextCompat.startForegroundService(context, intent)
            }
        }
    }

    private fun startTgProxy(context: Context) {
        if (TgProxyService.isRunning.value) return
        Log.i(TAG, "Starting TG proxy")
        val intent = Intent(context, TgProxyService::class.java)
        intent.action = TgProxyService.ACTION_START
        ContextCompat.startForegroundService(context, intent)
    }

    private fun stopTgProxy(context: Context) {
        if (!TgProxyService.isRunning.value) return
        Log.i(TAG, "Stopping TG proxy")
        val intent = Intent(context, TgProxyService::class.java)
        intent.action = TgProxyService.ACTION_STOP
        context.startService(intent)
    }

    fun restart(context: Context, mode: Mode) {
        if (appStatus.first == AppStatus.Running) {
            stop(context)
            scope.launch {
                val startTime = System.currentTimeMillis()
                while (System.currentTimeMillis() - startTime < 3000L) {
                    if (appStatus.first == AppStatus.Halted) break
                    delay(100)
                }
                start(context, mode)
            }
        }
    }
}
