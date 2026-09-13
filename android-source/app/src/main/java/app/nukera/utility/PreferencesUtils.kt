package app.nukera.utility

import android.content.Context
import android.content.SharedPreferences
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.PreferenceManager
import app.nukera.data.Mode

private const val DEFAULT_CMD_ARGS = "-d1 -d3+s -s6+s -d9+s -s12+s -d15+s -s20+s -d25+s -s30+s -d35+s -r1+s -S -a1 -As -d1 -d3+s -s6+s -d9+s -s12+s -d15+s -s20+s -d25+s -s30+s -d35+s -S -a1"

val PreferenceFragmentCompat.sharedPreferences
    get() = preferenceScreen.sharedPreferences

fun Context.getPreferences(): SharedPreferences =
    PreferenceManager.getDefaultSharedPreferences(this)

fun SharedPreferences.getIntStringNotNull(key: String, defValue: Int): Int =
    getString(key, defValue.toString())?.toIntOrNull() ?: defValue

fun SharedPreferences.getLongStringNotNull(key: String, defValue: Long): Long =
    getString(key, defValue.toString())?.toLongOrNull() ?: defValue

fun SharedPreferences.getStringNotNull(key: String, defValue: String): String =
    getString(key, defValue) ?: defValue

fun SharedPreferences.mode(): Mode =
    Mode.fromString(getStringNotNull("nukera_mode", "vpn"))

fun <T : Preference> PreferenceFragmentCompat.findPreferenceNotNull(key: CharSequence): T =
    findPreference(key) ?: throw IllegalStateException("Preference $key not found")

fun SharedPreferences.getSelectedApps(): List<String> {
    return getStringSet("selected_apps", emptySet())?.toList() ?: emptyList()
}

fun SharedPreferences.getCmdEnable(): Boolean {
    return getBoolean("nukera_enable_cmd_settings", true)
}

fun SharedPreferences.getCmdArgs(): String {
    return getStringNotNull("nukera_cmd_args", DEFAULT_CMD_ARGS)
}

fun SharedPreferences.checkIpAndPortInCmd(): Pair<String?, String?> {
    if (!getCmdEnable()) return Pair(null, null)
    val cmdArgs = shellSplit(getCmdArgs())

    fun getArgValue(argsList: List<String>, keys: List<String>): String? {
        for (i in argsList.indices) {
            val arg = argsList[i]
            for (key in keys) {
                if (key.startsWith("--")) {
                    if (arg == key && i + 1 < argsList.size) {
                        return argsList[i + 1]
                    } else if (arg.startsWith("$key=")) {
                        return arg.substringAfter('=')
                    }
                } else if (key.startsWith("-")) {
                    if (arg.startsWith(key) && arg.length > key.length) {
                        return arg.substring(key.length)
                    } else if (arg == key && i + 1 < argsList.size) {
                        return argsList[i + 1]
                    }
                }
            }
        }
        return null
    }

    val cmdIp = getArgValue(cmdArgs, listOf("--ip", "-i"))
    val cmdPort = getArgValue(cmdArgs, listOf("--port", "-p"))

    return Pair(cmdIp, cmdPort)
}


fun SharedPreferences.getProxyIpAndPort(): Pair<String, String> {
    val (cmdIp, cmdPort) = checkIpAndPortInCmd()

    val ip = cmdIp ?: getStringNotNull("nukera_proxy_ip", "127.0.0.1")
    val port = cmdPort ?: getStringNotNull("nukera_proxy_port", "1080")

    return Pair(ip, port)
}
