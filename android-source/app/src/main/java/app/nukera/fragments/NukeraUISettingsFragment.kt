package app.nukera.fragments

import android.content.SharedPreferences
import android.os.Bundle
import androidx.preference.*
import app.nukera.R
import app.nukera.data.UISettings
import app.nukera.data.UISettings.DesyncMethod.*
import app.nukera.data.UISettings.HostsMode.*
import app.nukera.utility.*

class NukeraUISettingsFragment : PreferenceFragmentCompat() {

    private val preferenceListener =
        SharedPreferences.OnSharedPreferenceChangeListener { _, _ ->
            updatePreferences()
        }

    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        setPreferencesFromResource(R.xml.nukera_ui_settings, rootKey)

        setEditTestPreferenceListenerInt(
            "nukera_max_connections",
            1,
            Short.MAX_VALUE.toInt()
        )
        setEditTestPreferenceListenerInt(
            "nukera_buffer_size",
            1,
            Int.MAX_VALUE / 4
        )
        setEditTestPreferenceListenerInt("nukera_default_ttl", 0, 255)
        setEditTestPreferenceListenerInt(
            "nukera_split_position",
            Int.MIN_VALUE,
            Int.MAX_VALUE
        )
        setEditTestPreferenceListenerInt("nukera_fake_ttl", 1, 255)
        setEditTestPreferenceListenerInt(
            "nukera_tlsrec_position",
            2 * Short.MIN_VALUE,
            2 * Short.MAX_VALUE,
        )

        findPreferenceNotNull<EditTextPreference>("nukera_oob_data")
            .setOnBindEditTextListener {
                it.filters = arrayOf(android.text.InputFilter.LengthFilter(1))
            }

        updatePreferences()
    }

    override fun onResume() {
        super.onResume()
        sharedPreferences?.registerOnSharedPreferenceChangeListener(preferenceListener)
    }

    override fun onPause() {
        super.onPause()
        sharedPreferences?.unregisterOnSharedPreferenceChangeListener(preferenceListener)
    }

    private fun updatePreferences() {
        val desyncMethod = findPreferenceNotNull<ListPreference>("nukera_desync_method").value.let { UISettings.DesyncMethod.fromName(it) }
        val hostsMode = findPreferenceNotNull<ListPreference>("nukera_hosts_mode").value.let { UISettings.HostsMode.fromName(it) }

        val hostsBlacklist = findPreferenceNotNull<EditTextPreference>("nukera_hosts_blacklist")
        val hostsWhitelist = findPreferenceNotNull<EditTextPreference>("nukera_hosts_whitelist")
        val desyncHttp = findPreferenceNotNull<CheckBoxPreference>("nukera_desync_http")
        val desyncHttps = findPreferenceNotNull<CheckBoxPreference>("nukera_desync_https")
        val desyncUdp = findPreferenceNotNull<CheckBoxPreference>("nukera_desync_udp")
        val splitPosition = findPreferenceNotNull<EditTextPreference>("nukera_split_position")
        val splitAtHost = findPreferenceNotNull<CheckBoxPreference>("nukera_split_at_host")
        val ttlFake = findPreferenceNotNull<EditTextPreference>("nukera_fake_ttl")
        val fakeSni = findPreferenceNotNull<EditTextPreference>("nukera_fake_sni")
        val fakeOffset = findPreferenceNotNull<EditTextPreference>("nukera_fake_offset")
        val oobChar = findPreferenceNotNull<EditTextPreference>("nukera_oob_data")
        val udpFakeCount = findPreferenceNotNull<EditTextPreference>("nukera_udp_fake_count")
        val hostMixedCase = findPreferenceNotNull<CheckBoxPreference>("nukera_host_mixed_case")
        val domainMixedCase = findPreferenceNotNull<CheckBoxPreference>("nukera_domain_mixed_case")
        val hostRemoveSpaces = findPreferenceNotNull<CheckBoxPreference>("nukera_host_remove_spaces")
        val splitTlsRec = findPreferenceNotNull<CheckBoxPreference>("nukera_tlsrec_enabled")
        val splitTlsRecPosition = findPreferenceNotNull<EditTextPreference>("nukera_tlsrec_position")
        val splitTlsRecAtSni = findPreferenceNotNull<CheckBoxPreference>("nukera_tlsrec_at_sni")

        hostsBlacklist.isVisible = hostsMode == Blacklist
        hostsWhitelist.isVisible = hostsMode == Whitelist

        val desyncEnabled = desyncMethod != None
        splitPosition.isVisible = desyncEnabled
        splitAtHost.isVisible = desyncEnabled

        val isFake = desyncMethod == Fake
        ttlFake.isVisible = isFake
        fakeSni.isVisible = isFake
        fakeOffset.isVisible = isFake

        val isOob = desyncMethod == OOB || desyncMethod == DISOOB
        oobChar.isVisible = isOob

        val desyncAllProtocols = !desyncHttp.isChecked && !desyncHttps.isChecked && !desyncUdp.isChecked

        val desyncHttpEnabled = desyncAllProtocols || desyncHttp.isChecked
        hostMixedCase.isEnabled = desyncHttpEnabled
        domainMixedCase.isEnabled = desyncHttpEnabled
        hostRemoveSpaces.isEnabled = desyncHttpEnabled

        val desyncUdpEnabled = desyncAllProtocols || desyncUdp.isChecked
        udpFakeCount.isEnabled = desyncUdpEnabled

        val desyncHttpsEnabled = desyncAllProtocols || desyncHttps.isChecked
        splitTlsRec.isEnabled = desyncHttpsEnabled
        val tlsRecEnabled = desyncHttpsEnabled && splitTlsRec.isChecked
        splitTlsRecPosition.isEnabled = tlsRecEnabled
        splitTlsRecAtSni.isEnabled = tlsRecEnabled
    }
}
