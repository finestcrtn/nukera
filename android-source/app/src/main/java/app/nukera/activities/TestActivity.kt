package app.nukera.activities

import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import app.nukera.R
import app.nukera.adapters.StrategyResultAdapter
import app.nukera.data.Mode
import app.nukera.data.AppStatus
import app.nukera.data.SiteResult
import app.nukera.data.StrategyResult
import app.nukera.services.appStatus
import app.nukera.services.ServiceManager
import app.nukera.utility.HistoryUtils
import app.nukera.utility.getPreferences
import app.nukera.utility.SiteCheckUtils
import app.nukera.utility.getIntStringNotNull
import app.nukera.utility.getLongStringNotNull
import androidx.core.content.edit
import app.nukera.utility.getStringNotNull
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import app.nukera.utility.DomainListUtils
import app.nukera.utility.getCmdArgs
import app.nukera.utility.mode
import kotlinx.coroutines.*
import java.io.File

class TestActivity : BaseActivity() {

    private lateinit var strategiesRecyclerView: RecyclerView
    private lateinit var progressTextView: TextView
    private lateinit var disclaimerTextView: TextView
    private lateinit var startStopButton: Button
    private lateinit var autoCombineButton: Button
    private lateinit var strategyAdapter: StrategyResultAdapter

    private lateinit var siteChecker: SiteCheckUtils
    private lateinit var cmdHistoryUtils: HistoryUtils
    private lateinit var sites: List<String>
    private lateinit var cmds: List<String>

    private var savedCmd: String = ""
    private var testJob: Job? = null
    private val strategies = mutableListOf<StrategyResult>()
    private val gson = Gson()

    private var isTesting: Boolean
        get() = prefs.getBoolean("is_test_running", false)
        set(value) {
            prefs.edit(commit = true) { putBoolean("is_test_running", value) }
        }

    private val prefs by lazy { getPreferences() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_proxy_test)
        setupToolbar()

        val ip = prefs.getStringNotNull("nukera_proxy_ip", "127.0.0.1")
        val port = prefs.getIntStringNotNull("nukera_proxy_port", 1080)

        siteChecker = SiteCheckUtils(ip, port)
        cmdHistoryUtils = HistoryUtils(this)

        strategiesRecyclerView = findViewById(R.id.strategiesRecyclerView)
        startStopButton = findViewById(R.id.startStopButton)
        autoCombineButton = findViewById(R.id.autoCombineButton)
        progressTextView = findViewById(R.id.progressTextView)
        disclaimerTextView = findViewById(R.id.disclaimerTextView)

        strategyAdapter = StrategyResultAdapter(this,
            onApply = { command ->
                addToHistory(command)
            }
        )

        strategiesRecyclerView.layoutManager = LinearLayoutManager(this)
        strategiesRecyclerView.adapter = strategyAdapter

        lifecycleScope.launch {
            val previousResults = loadResults()

            if (previousResults.isNotEmpty()) {
                progressTextView.text = getString(R.string.test_complete)
                disclaimerTextView.visibility = View.GONE

                strategies.clear()
                strategies.addAll(previousResults)

                strategyAdapter.updateStrategies(strategies)
            }

            if (isTesting) {
                progressTextView.text = getString(R.string.test_proxy_error)
                disclaimerTextView.text = getString(R.string.test_crash)
                disclaimerTextView.visibility = View.VISIBLE
                isTesting = false
            }
        }

        startStopButton.setOnClickListener {
            startStopButton.isClickable = false

            if (isTesting) {
                stopTesting()
            } else {
                startTesting()
            }

            startStopButton.postDelayed({ startStopButton.isClickable = true }, 1000)
        }

        autoCombineButton.setOnClickListener {
            autoCombine()
        }

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (isTesting) {
                    stopTesting()
                } else {
                    if (appStatus.first == AppStatus.Running) {
                        val intent = Intent(this@TestActivity, MainActivity::class.java)
                        intent.flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
                        startActivity(intent)
                    }
                }

                finish()
            }
        })

        supportActionBar?.setDisplayHomeAsUpEnabled(true)
    }

    override fun onCreateOptionsMenu(menu: Menu?): Boolean {
        menuInflater.inflate(R.menu.menu_test, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_copy_log -> {
                copyLog()
                true
            }
            R.id.action_settings -> {
                if (!isTesting) {
                    val intent = Intent(this, TestSettingsActivity::class.java)
                    startActivity(intent)
                } else {
                    Toast.makeText(this, R.string.settings_unavailable, Toast.LENGTH_SHORT).show()
                }
                true
            }
            android.R.id.home -> {
                onBackPressedDispatcher.onBackPressed()
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    private suspend fun waitForProxyStatus(statusNeeded: AppStatus): Boolean {
        val startTime = System.currentTimeMillis()
        while (System.currentTimeMillis() - startTime < 10000) {
            if (appStatus.first == statusNeeded) {
                delay(500)
                return true
            }
            delay(200)
        }
        return false
    }

    private suspend fun isProxyRunning(): Boolean = withContext(Dispatchers.IO) {
        appStatus.first == AppStatus.Running
    }

    private fun updateCmdArgs(cmd: String) {
        prefs.edit(commit = true) { putString("nukera_cmd_args", cmd) }
    }

    private fun startTesting() {
        sites = loadSites()
        cmds = loadCmds()

        if (sites.isEmpty()) {
            Toast.makeText(this, R.string.test_settings_domain_empty, Toast.LENGTH_LONG).show()
            return
        }

        testJob = lifecycleScope.launch(Dispatchers.IO) {
            isTesting = true
            savedCmd = prefs.getCmdArgs()

            strategies.clear()
            strategies.addAll(cmds.map { StrategyResult(command = it) })

            withContext(Dispatchers.Main) {
                disclaimerTextView.visibility = View.GONE

                window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                startStopButton.text = getString(R.string.test_stop)
                progressTextView.text = ""

                strategyAdapter.setTestingState(true)
                strategyAdapter.updateStrategies(strategies, sortByPercentage = false)
            }

            if (isProxyRunning()) {
                ServiceManager.stop(this@TestActivity)
                waitForProxyStatus(AppStatus.Halted)
            }

            val delaySec = prefs.getIntStringNotNull("nukera_proxytest_delay", 1)
            val requestsCount = prefs.getIntStringNotNull("nukera_proxytest_requests", 1)
            val requestTimeout = prefs.getLongStringNotNull("nukera_proxytest_timeout", 5)
            val requestLimit = prefs.getIntStringNotNull("nukera_proxytest_limit", 20)

            for (strategyIndex in strategies.indices) {
                if (!isActive) break

                val strategy = strategies[strategyIndex]
                val cmdIndex = strategyIndex + 1

                withContext(Dispatchers.Main) {
                    progressTextView.text = getString(R.string.test_process, cmdIndex, strategies.size)
                }

                updateCmdArgs(strategy.command)

                if (isProxyRunning()) {
                    ServiceManager.stop(this@TestActivity)
                    waitForProxyStatus(AppStatus.Halted)
                }

                ServiceManager.start(this@TestActivity, Mode.Proxy)

                if (!waitForProxyStatus(AppStatus.Running)) {
                    strategy.isCompleted = true
                    strategy.siteResults.add(SiteResult("PROXY_FAILED", 0, 0))
                    continue
                }

                delay(delaySec * 500L)

                val totalRequests = sites.size * requestsCount
                strategy.totalRequests = totalRequests

                withContext(Dispatchers.Main) {
                    strategyAdapter.notifyItemChanged(strategyIndex)
                }

                siteChecker.checkSitesAsync(
                    sites = sites,
                    requestsCount = requestsCount,
                    requestTimeout = requestTimeout,
                    concurrentRequests = requestLimit,
                    fullLog = true,
                    onSiteChecked = { site, successCount, countRequests ->
                        lifecycleScope.launch(Dispatchers.Main) {
                            strategy.currentProgress += countRequests
                            strategy.successCount += successCount
                            strategy.siteResults.add(SiteResult(site, successCount, countRequests))

                            strategyAdapter.notifyItemChanged(strategyIndex, "progress")
                        }
                    }
                )

                strategy.isCompleted = true

                withContext(Dispatchers.Main) {
                    strategyAdapter.notifyItemChanged(strategyIndex)
                    saveResults(strategies)
                }

                if (isProxyRunning()) {
                    ServiceManager.stop(this@TestActivity)
                    waitForProxyStatus(AppStatus.Halted)
                }

                delay(delaySec * 500L)
            }

            stopTesting()
        }
    }

    private fun stopTesting() {
        if (!isTesting) {
            return
        }

        lifecycleScope.launch(Dispatchers.IO) {
            isTesting = false
            updateCmdArgs(savedCmd)

            testJob?.cancel()
            testJob = null

            if (isProxyRunning()) {
                ServiceManager.stop(this@TestActivity)
            }

            filterDeadStrategies()
            removeBadStrategies(10)

            withContext(Dispatchers.Main) {
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                startStopButton.text = getString(R.string.test_start)
                progressTextView.text = getString(R.string.test_complete)
                autoCombineButton.visibility = View.VISIBLE

                strategyAdapter.setTestingState(false)
                strategyAdapter.updateStrategies(strategies, sortByPercentage = true)

                saveResults(strategies)
            }
        }
    }

    private fun addToHistory(command: String) {
        lifecycleScope.launch(Dispatchers.IO) {
            updateCmdArgs(command)
            cmdHistoryUtils.addCommand(command)

            val mode = prefs.mode()
            if (mode == Mode.VPN && VpnService.prepare(this@TestActivity) != null) return@launch

            val toastText = if (appStatus.first == AppStatus.Running) {
                ServiceManager.restart(this@TestActivity, mode)
                R.string.service_restart
            } else {
                R.string.cmd_history_applied
            }

            withContext(Dispatchers.Main) {
                Toast.makeText(this@TestActivity, toastText, Toast.LENGTH_SHORT).show()
            }
        }
    }

    private fun saveResults(results: List<StrategyResult>) {
        val file = File(filesDir, "proxy_test_results.json")
        val json = gson.toJson(results)
        file.writeText(json)
    }

    private fun filterDeadStrategies() {
        val before = strategies.size
        strategies.removeAll { strategy ->
            strategy.isCompleted && strategy.siteResults.all { it.successCount == 0 }
        }
        val removed = before - strategies.size
        if (removed > 0) {
            lifecycleScope.launch(Dispatchers.Main) {
                Toast.makeText(this@TestActivity, getString(R.string.test_dead_filtered, removed), Toast.LENGTH_SHORT).show()
                strategyAdapter.updateStrategies(strategies, sortByPercentage = true)
                saveResults(strategies)
            }
        }
    }

    private fun autoCombine() {
        val completedStrategies = strategies.filter { it.isCompleted && it.successCount > 0 }
        if (completedStrategies.isEmpty()) {
            Toast.makeText(this, R.string.test_complete_info, Toast.LENGTH_SHORT).show()
            return
        }

        val allSites = completedStrategies.flatMap { it.siteResults.map { sr -> sr.site } }.distinct()
        val coveredSites = mutableSetOf<String>()
        val selectedStrategies = mutableListOf<Pair<List<String>, StrategyResult>>()

        while (coveredSites.size < allSites.size) {
            val best = completedStrategies
                .filter { it !in selectedStrategies.map { p -> p.second } }
                .maxByOrNull { strategy ->
                    strategy.siteResults.count { it.successCount > 0 && it.site !in coveredSites }
                } ?: break

            val newSites = best.siteResults.filter { it.successCount > 0 && it.site !in coveredSites }.map { it.site }
            if (newSites.isEmpty()) break

            selectedStrategies.add(newSites to best)
            coveredSites.addAll(newSites)
        }

        if (selectedStrategies.isEmpty()) {
            Toast.makeText(this, R.string.test_complete_info, Toast.LENGTH_SHORT).show()
            return
        }

        val combined = StringBuilder()
        selectedStrategies.forEachIndexed { index, (domains, strategy) ->
            if (index > 0) combined.append(" -A ")
            combined.append("-H :${domains.joinToString(",")} ${strategy.command}")
        }

        val uncovered = allSites.filter { it !in coveredSites }
        if (uncovered.isNotEmpty()) {
            val bestOverall = completedStrategies.maxByOrNull { it.successPercentage } ?: completedStrategies.first()
            combined.append(" -A ")
            combined.append(bestOverall.command)
        }

        val result = combined.toString().trim()
        updateCmdArgs(result)

        lifecycleScope.launch(Dispatchers.Main) {
            Toast.makeText(this@TestActivity, R.string.test_auto_combine_result, Toast.LENGTH_LONG).show()

            val mode = prefs.mode()
            if (mode == Mode.VPN && VpnService.prepare(this@TestActivity) != null) return@launch
            ServiceManager.restart(this@TestActivity, mode)
        }
    }

    private fun loadResults(): List<StrategyResult> {
        val file = File(filesDir, "proxy_test_results.json")
        return if (file.exists()) {
            try {
                val json = file.readText()
                val type = object : TypeToken<List<StrategyResult>>() {}.type
                gson.fromJson<List<StrategyResult>>(json, type) ?: emptyList()
            } catch (e: Exception) {
                emptyList()
            }
        } else {
            emptyList()
        }
    }

    private fun loadSites(): List<String> {
        DomainListUtils.syncLists(this)
        return DomainListUtils.getActiveDomains(this)
    }

    private fun loadCmds(): List<String> {
        val userCommands = prefs.getBoolean("nukera_proxytest_usercommands", false)
        val sniValue = prefs.getStringNotNull("nukera_proxytest_sni", "google.com")

        return if (userCommands) {
            val content = prefs.getStringNotNull("nukera_proxytest_commands", "")
            content.replace("{sni}", "\"${sniValue}\"").lines().map { it.trim() }.filter { it.isNotEmpty() }
        } else {
            val internalFile = File(filesDir, "proxytest_strategies.list")
            if (!internalFile.exists()) {
                assets.open("proxytest_strategies.list").bufferedReader().use { input ->
                    internalFile.writeText(input.readText())
                }
            }
            internalFile.readText()
                .replace("{sni}", "\"${sniValue}\"")
                .lines().map { it.trim() }.filter { it.isNotEmpty() }
        }
    }

    private fun removeBadStrategies(threshold: Int = 10) {
        val internalFile = File(filesDir, "proxytest_strategies.list")
        if (!internalFile.exists()) return

        val allStrategies = internalFile.readText().lines().map { it.trim() }.filter { it.isNotEmpty() }
        val completedStrategies = strategies.filter { it.isCompleted }

        val badCommands = completedStrategies
            .filter { it.siteResults.count { sr -> sr.successCount > 0 } < threshold }
            .map { it.command }
            .toSet()

        if (badCommands.isEmpty()) return

        val filtered = allStrategies.filter { it !in badCommands }
        internalFile.writeText(filtered.joinToString("\n"))

        lifecycleScope.launch(Dispatchers.Main) {
            Toast.makeText(this@TestActivity, getString(R.string.test_dead_filtered, badCommands.size), Toast.LENGTH_SHORT).show()
        }
    }

    private fun copyLog() {
        val completeStrategies = strategies.filter { it.isCompleted }

        if (completeStrategies.isEmpty()) {
            Toast.makeText(this, R.string.toast_copied, Toast.LENGTH_SHORT).show()
            return
        }

        val sb = StringBuilder()

        completeStrategies.forEach { strategy ->
            sb.appendLine("${strategy.command}\n")

            strategy.siteResults.forEach { site ->
                sb.appendLine("${site.site} - ${site.successCount}/${site.totalCount}")
            }

            sb.appendLine("\n${strategy.successCount}/${strategy.totalRequests}")
            sb.appendLine("-------------")
        }

        val clipboard = getSystemService(CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val clip = android.content.ClipData.newPlainText("proxy_test_log", sb.toString())
        clipboard.setPrimaryClip(clip)

        Toast.makeText(this, R.string.toast_copied, Toast.LENGTH_SHORT).show()
    }
}
