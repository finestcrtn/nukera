package app.nukera.fragments

import android.content.SharedPreferences
import android.os.Bundle
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.preference.*
import app.nukera.R
import app.nukera.data.AppStatus
import app.nukera.data.Mode
import app.nukera.services.ServiceManager
import app.nukera.services.appStatus
import app.nukera.utility.*

class MainSettingsFragment : PreferenceFragmentCompat() {

    private val preferenceListener =
        SharedPreferences.OnSharedPreferenceChangeListener { _, _ ->
            updatePreferences()
        }

    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        setPreferencesFromResource(R.xml.main_settings, rootKey)

        findPreferenceNotNull<ListPreference>("language")
            .setOnPreferenceChangeListener { _, newValue ->
                SettingsUtils.setLang(newValue as String)
                true
            }

        findPreferenceNotNull<Preference>("current_strategy").apply {
            summary = requireContext().getPreferences().getCmdArgs()
            setOnPreferenceClickListener {
                showStrategyEditDialog(this)
                true
            }
        }

        updatePreferences()
    }

    override fun onResume() {
        super.onResume()
        sharedPreferences?.registerOnSharedPreferenceChangeListener(preferenceListener)
        updatePreferences()
    }

    override fun onPause() {
        super.onPause()
        sharedPreferences?.unregisterOnSharedPreferenceChangeListener(preferenceListener)
    }

    private fun showStrategyEditDialog(preference: Preference) {
        val currentCmd = requireContext().getPreferences().getCmdArgs()

        val input = EditText(requireContext()).apply {
            setText(currentCmd)
            setSelectAllOnFocus(true)
            isSingleLine = false
            minLines = 3
        }

        val container = LinearLayout(requireContext()).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(50, 20, 50, 20)
            addView(input)
        }

        AlertDialog.Builder(requireContext())
            .setTitle(R.string.current_strategy)
            .setMessage(R.string.current_strategy_summary)
            .setView(container)
            .setPositiveButton(android.R.string.ok) { _, _ ->
                val newCmd = input.text.toString().trim()
                if (newCmd.isNotBlank()) {
                    requireContext().getPreferences().edit()
                        .putString("nukera_cmd_args", newCmd)
                        .commit()
                    preference.summary = newCmd
                    if (appStatus.first == AppStatus.Running) {
                        val ctx = context ?: return@setPositiveButton
                        val mode = ctx.getPreferences().mode()
                        if (mode == Mode.VPN && android.net.VpnService.prepare(ctx) != null) return@setPositiveButton
                        ServiceManager.restart(ctx, mode)
                        Toast.makeText(ctx, R.string.service_restart, Toast.LENGTH_SHORT).show()
                    }
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun updatePreferences() {
        val strategyPref = findPreference<Preference>("current_strategy")
        strategyPref?.summary = requireContext().getPreferences().getCmdArgs()
    }
}
