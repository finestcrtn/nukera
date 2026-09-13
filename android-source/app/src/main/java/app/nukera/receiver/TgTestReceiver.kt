package app.nukera.receiver

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import app.nukera.utility.TgDownloader
import java.io.File

class TgTestReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val thread = Thread {
            val result = TgDownloader.run(context)
            try {
                File(context.cacheDir, "tg_test_result.txt").writeText(
                    "ts=${System.currentTimeMillis()} $result\n"
                )
            } catch (e: Exception) {
                Log.e("TgTest", "write result failed", e)
            }
        }
        thread.name = "tg-test"
        thread.start()
    }
}