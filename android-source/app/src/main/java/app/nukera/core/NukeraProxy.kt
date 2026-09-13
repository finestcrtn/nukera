package app.nukera.core

class NukeraProxy {
    companion object {
        init {
            System.loadLibrary("nukera")
        }
    }

    fun startProxy(preferences: NukeraProxyPreferences): Int {
        val args = prepareArgs(preferences)
        return jniStartProxy(args)
    }

    fun stopProxy(): Int {
        return jniStopProxy()
    }

    private fun prepareArgs(preferences: NukeraProxyPreferences): Array<String> =
        when (preferences) {
            is NukeraProxyCmdPreferences -> preferences.args
            is NukeraProxyUIPreferences -> preferences.uiargs
        }

    private external fun jniStartProxy(args: Array<String>): Int
    private external fun jniStopProxy(): Int
    external fun jniForceClose(): Int
}