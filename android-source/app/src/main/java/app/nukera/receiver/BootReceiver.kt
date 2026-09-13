package app.nukera.receiver

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.SystemClock
import app.nukera.data.Mode
import app.nukera.services.ServiceManager
import app.nukera.utility.getPreferences
import app.nukera.utility.mode

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED ||
            intent.action == Intent.ACTION_REBOOT ||
            intent.action == "android.intent.action.QUICKBOOT_POWERON") {

            // for A15, todo: use wasForceStopped
            if (SystemClock.elapsedRealtime() > 5 * 60 * 1000) {
                return
            }

            val preferences = context.getPreferences()
            val autorunEnabled = preferences.getBoolean("autostart", false)

            if(autorunEnabled) {
                when (preferences.mode()) {
                    Mode.VPN -> {
                        if (VpnService.prepare(context) == null) {
                            ServiceManager.start(context, Mode.VPN)
                        }
                    }

                    Mode.Proxy -> ServiceManager.start(context, Mode.Proxy)
                }
            }
        }
    }
}