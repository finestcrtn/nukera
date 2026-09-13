package app.nukera.data

import android.content.SharedPreferences

data class UISettings(
    val ip: String = "127.0.0.1",
    val port: Int = 1080,
    val maxConnections: Int = 512,
    val bufferSize: Int = 16384,
    val defaultTtl: Int = 0,
    val noDomain: Boolean = false,
    val desyncHttp: Boolean = true,
    val desyncHttps: Boolean = true,
    val desyncUdp: Boolean = true,
    val desyncMethod: DesyncMethod = DesyncMethod.OOB,
    val splitPosition: Int = 1,
    val splitAtHost: Boolean = false,
    val fakeTtl: Int = 8,
    val fakeSni: String = "www.iana.org",
    val oobChar: String = "a",
    val hostMixedCase: Boolean = false,
    val domainMixedCase: Boolean = false,
    val hostRemoveSpaces: Boolean = false,
    val tlsRecordSplit: Boolean = false,
    val tlsRecordSplitPosition: Int = 0,
    val tlsRecordSplitAtSni: Boolean = false,
    val hostsMode: HostsMode = HostsMode.Disable,
    val hosts: String? = null,
    val tcpFastOpen: Boolean = false,
    val udpFakeCount: Int = 1,
    val dropSack: Boolean = false,
    val fakeOffset: Int = 0,
) {
    enum class DesyncMethod {
        None, Split, Disorder, Fake, OOB, DISOOB;

        companion object {
            fun fromName(name: String): DesyncMethod = when (name) {
                "none" -> None
                "split" -> Split
                "disorder" -> Disorder
                "fake" -> Fake
                "oob" -> OOB
                "disoob" -> DISOOB
                else -> throw IllegalArgumentException("Unknown desync method: $name")
            }
        }
    }

    enum class HostsMode {
        Disable, Blacklist, Whitelist;

        companion object {
            fun fromName(name: String): HostsMode = when (name) {
                "disable" -> Disable
                "blacklist" -> Blacklist
                "whitelist" -> Whitelist
                else -> throw IllegalArgumentException("Unknown hosts mode: $name")
            }
        }
    }

    companion object {
        fun fromSharedPreferences(preferences: SharedPreferences): UISettings {
            val hostsMode = preferences.getString("nukera_hosts_mode", null) ?.let { HostsMode.fromName(it) } ?: HostsMode.Disable

            val hosts = when (hostsMode) {
                HostsMode.Blacklist -> preferences.getString("nukera_hosts_blacklist", null)
                HostsMode.Whitelist -> preferences.getString("nukera_hosts_whitelist", null)
                else -> null
            }

            return UISettings(
                ip = preferences.getString("nukera_proxy_ip", null) ?: "127.0.0.1",
                port = preferences.getString("nukera_proxy_port", null)?.toIntOrNull() ?: 1080,
                maxConnections = preferences.getString("nukera_max_connections", null)?.toIntOrNull() ?: 512,
                bufferSize = preferences.getString("nukera_buffer_size", null)?.toIntOrNull() ?: 16384,
                defaultTtl = preferences.getString("nukera_default_ttl", null)?.toIntOrNull() ?: 0,
                noDomain = preferences.getBoolean("nukera_no_domain", false),
                desyncHttp = preferences.getBoolean("nukera_desync_http", true),
                desyncHttps = preferences.getBoolean("nukera_desync_https", true),
                desyncUdp = preferences.getBoolean("nukera_desync_udp", true),
                desyncMethod = preferences.getString("nukera_desync_method", null) ?.let { DesyncMethod.fromName(it) } ?: DesyncMethod.OOB,
                splitPosition = preferences.getString("nukera_split_position", null)?.toIntOrNull() ?: 1,
                splitAtHost = preferences.getBoolean("nukera_split_at_host", false),
                fakeTtl = preferences.getString("nukera_fake_ttl", null)?.toIntOrNull() ?: 8,
                fakeSni = preferences.getString("nukera_fake_sni", null) ?: "www.iana.org",
                oobChar = preferences.getString("nukera_oob_data", null) ?: "a",
                hostMixedCase = preferences.getBoolean("nukera_host_mixed_case", false),
                domainMixedCase = preferences.getBoolean("nukera_domain_mixed_case", false),
                hostRemoveSpaces = preferences.getBoolean("nukera_host_remove_spaces", false),
                tlsRecordSplit = preferences.getBoolean("nukera_tlsrec_enabled", false),
                tlsRecordSplitPosition = preferences.getString("nukera_tlsrec_position", null)?.toIntOrNull() ?: 0,
                tlsRecordSplitAtSni = preferences.getBoolean("nukera_tlsrec_at_sni", false),
                tcpFastOpen = preferences.getBoolean("nukera_tcp_fast_open", false),
                udpFakeCount = preferences.getString("nukera_udp_fake_count", null)?.toIntOrNull() ?: 1,
                dropSack = preferences.getBoolean("nukera_drop_sack", false),
                fakeOffset = preferences.getString("nukera_fake_offset", null)?.toIntOrNull() ?: 0,
                hostsMode = hostsMode,
                hosts = hosts,
            )
        }
    }
}