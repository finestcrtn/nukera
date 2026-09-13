package app.nukera.activities

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.widget.ScrollView
import android.widget.TextView
import android.view.animation.LinearInterpolator
import android.widget.Toast
import androidx.activity.result.ActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AlertDialog
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.lifecycle.lifecycleScope
import app.nukera.R
import app.nukera.data.*
import app.nukera.databinding.ActivityMainBinding
import app.nukera.services.ServiceManager
import app.nukera.services.TgProxyService
import app.nukera.services.appStatus
import app.nukera.utility.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import kotlin.system.exitProcess

class MainActivity : BaseActivity() {
    private lateinit var binding: ActivityMainBinding
    private var optimizeJob: Job? = null
    @Volatile private var isOptimizing = false
    private var bestCmd = ""
    private var bestScore = 0
    private var savedCmd = ""

    companion object {
        private val TAG: String = MainActivity::class.java.simpleName
        private const val BATTERY_OPTIMIZATION_REQUESTED = "battery_optimization_requested"

        private fun collectLogs(): String? {
            return try {
                val process = Runtime.getRuntime().exec("logcat *:D -d")
                process.inputStream.bufferedReader().use { reader ->
                    reader.readText()
                }
            } catch (exception: Exception) {
                Log.e(TAG, "Failed to collect logs", exception)
                null
            }
        }
    }

    private val vpnRegister =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
            if (it.resultCode == RESULT_OK) {
                ServiceManager.start(this, Mode.VPN)
            } else {
                Toast.makeText(this, R.string.vpn_permission_denied, Toast.LENGTH_SHORT).show()
                updateStatus()
            }
        }

    private val logsRegister = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
        ::handleLogsFileResult
    )

    private fun handleLogsFileResult(result: ActivityResult) {
        if (result.resultCode != RESULT_OK) return
        val data = result.data ?: return
        val uri = data.data
        val path = data.getStringExtra(FileActivity.EXTRA_PATH)
        val file = if (path == null) null else File(path)
        if (uri == null && file == null) return

        lifecycleScope.launch(Dispatchers.IO) {
            val logs = collectLogs()
            if (logs == null) {
                runOnUiThread {
                    Toast.makeText(this@MainActivity, R.string.logs_failed, Toast.LENGTH_SHORT).show()
                }
                return@launch
            }

            try {
                val outputStream = when {
                    uri != null -> contentResolver.openOutputStream(uri)
                    file != null -> file.outputStream()
                    else -> null
                }
                if (outputStream == null) {
                    Log.e(TAG, "Failed to open output stream")
                } else {
                    outputStream.use { stream ->
                        stream.write(logs.toByteArray())
                    }
                }
            } catch (exception: Exception) {
                Log.e(TAG, "Failed to save logs", exception)
            }
        }
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            Log.d(TAG, "Received intent: ${intent?.action}")

            if (intent == null) {
                Log.w(TAG, "Received null intent")
                return
            }

            val senderOrd = intent.getIntExtra(SENDER, -1)
            val sender = Sender.entries.getOrNull(senderOrd)
            if (sender == null) {
                Log.w(TAG, "Received intent with unknown sender: $senderOrd")
                return
            }

            when (val action = intent.action) {
                STARTED_BROADCAST,
                STOPPED_BROADCAST -> updateStatus()

                FAILED_BROADCAST -> {
                    Toast.makeText(
                        context,
                        getString(R.string.failed_to_start, sender.name),
                        Toast.LENGTH_SHORT,
                    ).show()
                    updateStatus()
                }

                else -> Log.w(TAG, "Unknown action: $action")
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        setupToolbar()

        val intentFilter = IntentFilter().apply {
            addAction(STARTED_BROADCAST)
            addAction(STOPPED_BROADCAST)
            addAction(FAILED_BROADCAST)
        }

        @SuppressLint("UnspecifiedRegisterReceiverFlag")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(receiver, intentFilter, RECEIVER_EXPORTED)
        } else {
            registerReceiver(receiver, intentFilter)
        }

        binding.statusButtonCard.setOnClickListener {
            if (isOptimizing) return@setOnClickListener
            binding.statusButtonCard.isClickable = false

            val (status, _) = appStatus
            when (status) {
                AppStatus.Halted -> start()
                AppStatus.Running -> stop()
            }

            binding.statusButtonCard.postDelayed({
                binding.statusButtonCard.isClickable = true
            }, 1000)
        }

        binding.settingsButton.setOnClickListener {
            val (status, _) = appStatus
            if (isOptimizing) {
                Toast.makeText(this, R.string.stop_before_settings, Toast.LENGTH_SHORT).show()
                return@setOnClickListener
            }
            if (status == AppStatus.Halted) {
                startActivity(Intent(this, SettingsActivity::class.java))
            } else {
                Toast.makeText(this, R.string.stop_before_settings, Toast.LENGTH_SHORT).show()
            }
        }

        binding.optimizeButton?.setOnClickListener {
            if (isOptimizing) {
                stopOptimize()
            } else {
                startOptimize()
            }
        }

        binding.telegramButton?.setOnClickListener {
            val url = TgProxyService.getProxyUrl(this)
            if (url.isNotEmpty()) {
                try {
                    val intent = Intent(Intent.ACTION_VIEW, android.net.Uri.parse(url))
                    startActivity(intent)
                } catch (e: Exception) {
                    Toast.makeText(this, "Telegram not installed", Toast.LENGTH_SHORT).show()
                }
            } else {
                Toast.makeText(this, "TG proxy not ready", Toast.LENGTH_SHORT).show()
            }
        }

        if (!PermissionUtils.hasNotificationPermission(this)) {
            PermissionUtils.requestNotificationPermission(this, 1)
        } else {
            requestBatteryOptimization()
        }

        if (getPreferences().getBoolean("auto_connect", false) && appStatus.first != AppStatus.Running) {
            this.start()
        }

        lifecycleScope.launch {
            TgProxyService.isRunning.collect { running ->
                val (status, _) = appStatus
                binding.telegramButton?.visibility = if (status == AppStatus.Running && running) View.VISIBLE else View.GONE
            }
        }
    }

    override fun onResume() {
        super.onResume()
        updateStatus()
    }

    override fun onDestroy() {
        super.onDestroy()
        unregisterReceiver(receiver)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)

        if (requestCode == 1) {
            requestBatteryOptimization()
        }
    }

    override fun onCreateOptionsMenu(menu: Menu?): Boolean {
        return false
    }

    private fun showDiagnostics() {
        val report = DiagnosticUtils.buildReport(this)
        val padding = (24 * resources.displayMetrics.density).toInt()

        val textView = TextView(this).apply {
            text = report
            setPadding(padding, padding / 2, padding, padding / 2)
        }

        val scrollView = ScrollView(this).apply {
            addView(textView)
        }

        val dialog = AlertDialog.Builder(this)
            .setTitle(R.string.diagnostics)
            .setView(scrollView)
            .setPositiveButton(R.string.diagnostic_copy) { _, _ ->
                ClipboardUtils.copy(this, report, getString(R.string.diagnostics))
            }
            .setNegativeButton(android.R.string.cancel, null)
            .create()

        dialog.show()
    }

    private fun start() {
        when (getPreferences().mode()) {
            Mode.VPN -> {
                val intentPrepare = VpnService.prepare(this)
                if (intentPrepare != null) {
                    vpnRegister.launch(intentPrepare)
                } else {
                    ServiceManager.start(this, Mode.VPN)
                }
            }

            Mode.Proxy -> ServiceManager.start(this, Mode.Proxy)
        }
    }

    private fun stop() {
        ServiceManager.stop(this)
    }

    private fun updateStatus() {
        Log.d(TAG, "updateStatus called, isOptimizing=$isOptimizing")
        if (isOptimizing) return

        val (status, mode) = appStatus

        Log.i(TAG, "Updating status: $status, $mode")

        val preferences = getPreferences()

        when (status) {
            AppStatus.Halted -> {
                binding.statusButtonCard.setBackgroundResource(R.drawable.hero_core_off)
                binding.statusButtonIcon.clearColorFilter()
                binding.heroAmbientGlow?.setBackgroundResource(R.drawable.hero_glow_off)

                when (preferences.mode()) {
                    Mode.VPN -> binding.statusText.setText(R.string.vpn_disconnected)
                    Mode.Proxy -> binding.statusText.setText(R.string.proxy_down)
                }
            }

            AppStatus.Running -> {
                binding.statusButtonCard.setBackgroundResource(R.drawable.hero_core_on)
                binding.statusButtonIcon.setColorFilter(ContextCompat.getColor(this, android.R.color.white))
                binding.heroAmbientGlow?.setBackgroundResource(R.drawable.hero_glow_on)
                startGlowPulse()

                when (mode) {
                    Mode.VPN -> binding.statusText.setText(R.string.vpn_connected)
                    Mode.Proxy -> binding.statusText.setText(R.string.proxy_up)
                }
            }
        }

        binding.telegramButton?.visibility = if (status == AppStatus.Running && TgProxyService.isRunning.value) View.VISIBLE else View.GONE
    }

    private fun startGlowPulse() {
        val glow = binding.heroAmbientGlow ?: return
        glow.tag?.let { (it as? android.animation.ObjectAnimator)?.cancel() }
        val pulse = android.animation.ObjectAnimator.ofFloat(glow, "alpha", 0.6f, 1.0f).apply {
            duration = 2000
            repeatMode = android.animation.ObjectAnimator.REVERSE
            repeatCount = android.animation.ObjectAnimator.INFINITE
        }
        glow.tag = pulse
        pulse.start()
    }

    private fun stopGlowPulse() {
        val glow = binding.heroAmbientGlow ?: return
        val pulse = glow.tag as? android.animation.ObjectAnimator
        pulse?.cancel()
        glow.tag = null
        glow.alpha = 1.0f
    }

    private fun startSpinningGear() {
        Log.d(TAG, "Starting spinning gear")
        binding.statusButtonIcon.setImageResource(R.drawable.baseline_settings_24)
        binding.statusButtonIcon.setColorFilter(ContextCompat.getColor(this, android.R.color.white))
        binding.statusButtonIcon.contentDescription = getString(R.string.optimize)

        val animation = android.animation.ObjectAnimator.ofFloat(
            binding.statusButtonIcon, "rotation", 0f, 360f
        )
        animation.duration = 2000
        animation.repeatCount = android.animation.ObjectAnimator.INFINITE
        animation.interpolator = LinearInterpolator()
        binding.statusButtonIcon.tag = animation
        animation.start()

        binding.statusButtonCard.setBackgroundResource(R.drawable.hero_core_optimizing)
        binding.heroAmbientGlow?.setBackgroundResource(R.drawable.hero_glow_optimizing)
        startGlowPulse()
        binding.statusText.text = getString(R.string.optimize)
        Log.d(TAG, "Status text set to: ${binding.statusText.text}")
    }

    private fun stopSpinningGear() {
        val animation = binding.statusButtonIcon.tag as? android.animation.ObjectAnimator
        animation?.cancel()
        binding.statusButtonIcon.tag = null
        binding.statusButtonIcon.rotation = 0f
        binding.statusButtonIcon.setImageResource(R.drawable.ic_power)
        binding.statusButtonIcon.clearColorFilter()
        binding.statusButtonIcon.contentDescription = getString(R.string.vpn_connect)
        stopGlowPulse()
    }

    private fun startOptimize() {
        val sites = loadSites()
        val cmds = loadCmds()

        if (sites.isEmpty() || cmds.isEmpty()) {
            Toast.makeText(this, R.string.test_settings_domain_empty, Toast.LENGTH_SHORT).show()
            return
        }

        isOptimizing = true
        bestCmd = ""
        bestScore = 0
        savedCmd = getPreferences().getCmdArgs()

        binding.optimizeText?.text = getString(R.string.test_stop)
        binding.optimizeIcon?.setImageResource(android.R.drawable.ic_media_pause)
        binding.optimizeProgressRow?.visibility = View.VISIBLE
        binding.optimizeProgressBar?.max = cmds.size
        binding.optimizeProgressBar?.progress = 0

        startSpinningGear()
        binding.statusText.post {
            binding.statusText.text = getString(R.string.optimize)
        }

        if (appStatus.first == AppStatus.Running) {
            ServiceManager.stop(this)
        }

        optimizeJob = lifecycleScope.launch(Dispatchers.IO) {
            val siteChecker = SiteCheckUtils("127.0.0.1", "1080".toInt())
            val sniValue = getPreferences().getStringNotNull("nukera_proxytest_sni", "google.com")
            val requestTimeout = getPreferences().getLongStringNotNull("nukera_proxytest_timeout", 5)
            val requestsCount = getPreferences().getIntStringNotNull("nukera_proxytest_requests", 1)
            val requestLimit = getPreferences().getIntStringNotNull("nukera_proxytest_limit", 20)
            val delaySec = getPreferences().getIntStringNotNull("nukera_proxytest_delay", 1)

            for ((index, cmd) in cmds.withIndex()) {
                if (!isActive) break

                withContext(Dispatchers.Main) {
                    if (!isOptimizing) return@withContext
                    binding.optimizeProgressBar?.progress = index + 1
                    binding.optimizeProgressText?.text = getString(R.string.optimize_progress, index + 1, cmds.size)
                }

                val resolvedCmd = cmd.replace("{sni}", "\"${sniValue}\"")

                getPreferences().edit(commit = true) { putString("nukera_cmd_args", resolvedCmd) }

                if (!isActive) break

                if (appStatus.first == AppStatus.Running) {
                    ServiceManager.stop(this@MainActivity)
                    delay(1000)
                }

                ServiceManager.start(this@MainActivity, Mode.Proxy)

                var waited = 0
                while (appStatus.first != AppStatus.Running && waited < 10000) {
                    delay(200)
                    waited += 200
                }

                if (appStatus.first != AppStatus.Running) continue

                delay(delaySec * 500L)

                var score = 0
                val results = siteChecker.checkSitesAsync(
                    sites = sites,
                    requestsCount = requestsCount,
                    requestTimeout = requestTimeout,
                    concurrentRequests = requestLimit,
                    fullLog = false,
                    onSiteChecked = null
                )

                for ((_, successCount) in results) {
                    if (successCount > 0) score++
                }

                if (score > bestScore) {
                    bestScore = score
                    bestCmd = resolvedCmd
                }

                if (appStatus.first == AppStatus.Running) {
                    ServiceManager.stop(this@MainActivity)
                    delay(1000)
                }

                delay(delaySec * 500L)
            }

            val completed = isActive

            withContext(Dispatchers.Main) {
                isOptimizing = false
                stopSpinningGear()

                if (completed && bestCmd.isNotEmpty()) {
                    getPreferences().edit(commit = true) { putString("nukera_cmd_args", bestCmd) }
                    Toast.makeText(this@MainActivity, R.string.optimize_complete, Toast.LENGTH_SHORT).show()
                } else if (!completed) {
                    if (savedCmd.isNotEmpty()) {
                        getPreferences().edit(commit = true) { putString("nukera_cmd_args", savedCmd) }
                    }
                }

                binding.optimizeText?.text = getString(R.string.optimize)
                binding.optimizeIcon?.setImageResource(R.drawable.ic_speed)
                binding.optimizeProgressRow?.visibility = View.GONE

                updateStatus()
            }
        }
    }

    private fun stopOptimize() {
        if (!isOptimizing) return

        isOptimizing = false
        stopSpinningGear()

        if (savedCmd.isNotEmpty()) {
            getPreferences().edit(commit = true) { putString("nukera_cmd_args", savedCmd) }
        }

        binding.optimizeText?.text = getString(R.string.optimize)
        binding.optimizeIcon?.setImageResource(R.drawable.ic_speed)
        binding.optimizeProgressRow?.visibility = View.GONE
        binding.statusText.setText(R.string.vpn_disconnected)

        optimizeJob?.cancel()
        optimizeJob = null

        lifecycleScope.launch(Dispatchers.IO) {
            if (appStatus.first == AppStatus.Running) {
                ServiceManager.stop(this@MainActivity)
            }
            withContext(Dispatchers.Main) {
                updateStatus()
            }
        }
    }

    private fun loadSites(): List<String> {
        DomainListUtils.syncLists(this)
        return DomainListUtils.getActiveDomains(this)
    }

    private fun loadCmds(): List<String> {
        val internalFile = File(filesDir, "proxytest_strategies.list")
        if (!internalFile.exists()) {
            assets.open("proxytest_strategies.list").bufferedReader().use { input ->
                internalFile.writeText(input.readText())
            }
        }
        val sniValue = getPreferences().getStringNotNull("nukera_proxytest_sni", "google.com")
        return internalFile.readText()
            .replace("{sni}", "\"${sniValue}\"")
            .lines().map { it.trim() }.filter { it.isNotEmpty() }
    }

    private fun requestBatteryOptimization() {
        val preferences = getPreferences()
        val alreadyRequested = preferences.getBoolean(BATTERY_OPTIMIZATION_REQUESTED, false)

        if (!alreadyRequested && !PermissionUtils.isBatteryOptimizationDisabled(this)) {
            PermissionUtils.requestBatteryOptimization(this)
            preferences.edit { putBoolean(BATTERY_OPTIMIZATION_REQUESTED, true) }
        }
    }
}
