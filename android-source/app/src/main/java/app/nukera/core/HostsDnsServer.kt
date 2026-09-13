package app.nukera.core

import android.net.VpnService
import android.util.Log
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

class HostsDnsServer {
    companion object {
        private const val TAG = "HostsDnsServer"
        private const val VIRTUAL_DNS_IP = "10.10.10.2"
        private const val UPSTREAM_DNS = "1.1.1.1"
    }

    private val hostMapping = ConcurrentHashMap<String, String>()
    private var vpnService: VpnService? = null
    @Volatile private var isRunning = false
    private var clientSocket: DatagramSocket? = null
    private var upstreamSocket: DatagramSocket? = null
    private var serverThread: Thread? = null

    fun setVpnService(svc: VpnService) {
        vpnService = svc
    }

    fun loadHosts(assets: android.content.res.AssetManager) {
        try {
            assets.open("telegram-web.txt").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val trimmed = line.trim()
                    if (trimmed.isEmpty() || trimmed.startsWith("#")) return@forEach
                    val parts = trimmed.split("\\s+".toRegex(), limit = 2)
                    if (parts.size == 2) hostMapping[parts[1].lowercase()] = parts[0]
                }
            }
            Log.i(TAG, "Loaded ${hostMapping.size} hosts entries")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to load hosts", e)
            // fallback single mapping
            hostMapping["web.telegram.org"] = "149.154.167.220"
        }
    }

    fun start(): Boolean {
        if (isRunning) return true
        return try {
            clientSocket = DatagramSocket(53, InetAddress.getByName(VIRTUAL_DNS_IP))
            upstreamSocket = DatagramSocket()
            vpnService?.protect(upstreamSocket!!)
            upstreamSocket?.soTimeout = 3000
            isRunning = true
            serverThread = Thread({
                val buf = ByteArray(1024)
                Log.i(TAG, "DNS server started on $VIRTUAL_DNS_IP:53")
                while (isRunning) {
                    try {
                        val pkt = DatagramPacket(buf, buf.size)
                        clientSocket?.receive(pkt)
                        val domain = parseDomainFromDnsQuery(pkt.data, pkt.length)
                        val lower = domain?.lowercase()
                        if (lower != null && hostMapping.containsKey(lower)) {
                            val customIp = hostMapping[lower]!!
                            Log.i(TAG, "DNS override: $lower -> $customIp")
                            val resp = buildSyntheticDnsResponse(pkt.data, pkt.length, customIp)
                            clientSocket?.send(DatagramPacket(resp, resp.size, pkt.address, pkt.port))
                        } else {
                            forwardQueryToUpstream(pkt)
                        }
                    } catch (e: Exception) {
                        if (!isRunning) break
                        Log.e(TAG, "DNS handler error", e)
                    }
                }
            }, "HostsDnsServer").apply { isDaemon = true; start() }
            true
        } catch (e: Exception) {
            Log.e(TAG, "Failed to bind DNS server to $VIRTUAL_DNS_IP:53", e)
            stop()
            false
        }
    }

    private fun forwardQueryToUpstream(inPkt: DatagramPacket) {
        try {
            val upstreamAddr = InetAddress.getByName(UPSTREAM_DNS)
            val q = DatagramPacket(inPkt.data, inPkt.length, upstreamAddr, 53)
            upstreamSocket?.send(q)
            val respBuf = ByteArray(1024)
            val respPkt = DatagramPacket(respBuf, respBuf.size)
            upstreamSocket?.receive(respPkt)
            clientSocket?.send(DatagramPacket(respPkt.data, respPkt.length, inPkt.address, inPkt.port))
        } catch (e: Exception) {
            Log.e(TAG, "forward failed", e)
        }
    }

    fun stop() {
        isRunning = false
        try { clientSocket?.close() } catch (_: Exception) {}
        try { upstreamSocket?.close() } catch (_: Exception) {}
        clientSocket = null
        upstreamSocket = null
        serverThread?.interrupt()
        serverThread = null
        Log.i(TAG, "DNS server stopped")
    }

    fun parseDomainFromDnsQuery(data: ByteArray, length: Int): String? {
        if (length < 12) return null
        var pos = 12
        val sb = StringBuilder()
        while (pos < length) {
            val len = data[pos].toInt() and 0xFF
            if (len == 0) break
            if (len > 63 || pos + len >= length) return null
            if ((len and 0xC0) == 0xC0) break // compression not expected in query
            if (sb.isNotEmpty()) sb.append(".")
            sb.append(String(data, pos + 1, len, Charsets.US_ASCII))
            pos += len + 1
        }
        return if (sb.isEmpty()) null else sb.toString()
    }

    fun buildSyntheticDnsResponse(request: ByteArray, reqLen: Int, targetIp: String): ByteArray {
        val ipBytes = InetAddress.getByName(targetIp).address
        val resp = ByteArray(reqLen + 16)
        System.arraycopy(request, 0, resp, 0, reqLen)
        resp[2] = 0x81.toByte()
        resp[3] = 0x80.toByte()
        resp[6] = 0x00.toByte()
        resp[7] = 0x01.toByte()
        var pos = reqLen
        resp[pos++] = 0xC0.toByte()
        resp[pos++] = 0x0C.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x01.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x01.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x3C.toByte()
        resp[pos++] = 0x00.toByte()
        resp[pos++] = 0x04.toByte()
        System.arraycopy(ipBytes, 0, resp, pos, 4)
        return resp.copyOf(pos + 4)
    }
}
