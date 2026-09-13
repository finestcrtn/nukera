package app.nukera.activities

import android.app.Activity
import android.content.SharedPreferences
import android.net.VpnService
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.content.edit
import app.nukera.data.AppStatus
import app.nukera.data.Mode
import app.nukera.services.ServiceManager
import app.nukera.services.appStatus
import app.nukera.utility.getCmdArgs
import app.nukera.utility.getPreferences
import app.nukera.utility.mode

class ToggleActivity : Activity() {

    companion object {
        private const val TAG = "ToggleServiceActivity"
    }

    private lateinit var prefs: SharedPreferences

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        prefs = getPreferences()
        val strategy = intent.getStringExtra("strategy")
        val updated = updateStrategy(strategy)

        val onlyUpdate = intent.getBooleanExtra("only_update", false)
        val onlyStart = intent.getBooleanExtra("only_start", false)
        val onlyStop = intent.getBooleanExtra("only_stop", false)

        when {
            onlyUpdate -> {
                Log.i(TAG, "Only update strategy")
            }
            onlyStart -> {
                val (status) = appStatus
                if (status == AppStatus.Halted) {
                    startService()
                } else {
                    Log.i(TAG, "Service already running")
                }
            }
            onlyStop -> {
                val (status) = appStatus
                if (status == AppStatus.Running) {
                    stopService()
                } else {
                    Log.i(TAG, "Service already stopped")
                }
            }
            else -> {
                toggleService(updated)
            }
        }

        finish()
    }

    private fun startService() {
        val mode = prefs.mode()

        if (mode == Mode.VPN && VpnService.prepare(this) != null) {
            return
        }

        ServiceManager.start(this, mode)
        Log.i(TAG, "Toggle service start")
    }

    private fun restartService() {
        val mode = prefs.mode()

        if (mode == Mode.VPN && VpnService.prepare(this) != null) {
            return
        }

        ServiceManager.restart(this, mode)
        Log.i(TAG, "Toggle service start")
    }

    private fun stopService() {
        ServiceManager.stop(this)
        Log.i(TAG, "Toggle service stop")
    }

    private fun toggleService(restart: Boolean) {
        val (status) = appStatus
        when (status) {
            AppStatus.Halted -> {
                startService()
            }
            AppStatus.Running -> {
                if (restart) {
                    restartService()
                } else {
                    stopService()
                }
            }
        }
    }

    private fun updateStrategy(strategy: String?): Boolean {
        val current = prefs.getCmdArgs()
        if (strategy != null && strategy != current) {
            prefs.edit(commit = true) { putString("nukera_cmd_args", strategy) }
            Log.i(TAG, "Strategy updated to: $strategy")
            return true
        }
        return false
    }
}
