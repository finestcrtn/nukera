package app.nukera.utility

import android.content.Context
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.security.MessageDigest
import java.security.cert.X509Certificate
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLParameters
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory
import javax.net.ssl.X509TrustManager

object TgDownloader {
    const val TEST_URL_PATH = "/file/400780400366/6/dI0zgi0mM5s.323818.webp/6a362dadee6e7e6011"

    fun run(context: Context): String {
        val proxyPort = try {
            context.getSharedPreferences("app.nukera_preferences", Context.MODE_PRIVATE)
                .getString("nukera_proxy_port", "1080")?.toInt() ?: 1080
        } catch (e: Exception) {
            1080
        }
        var sock: Socket? = null
        var ssl: SSLSocket? = null
        try {
            // 1) SOCKS5 to byedpi, CONNECT 149.154.167.220:443 (= hev redirect target)
            sock = Socket()
            sock.connect(InetSocketAddress("127.0.0.1", proxyPort), 8000)
            sock.tcpNoDelay = true
            val o: OutputStream = sock.getOutputStream()
            val i: InputStream = sock.getInputStream()
            o.write(byteArrayOf(0x05, 0x01, 0x00)); o.flush()
            if (readN(i, 2)[1] != 0.toByte()) return "FAIL socks-greet"
            val cmd = byteArrayOf(
                0x05, 0x01, 0x00, 0x01,
                0x95.toByte(), 0x9A.toByte(), 0xA7.toByte(), 0xDC.toByte(),
                0x01, 0xBB.toByte()
            )
            o.write(cmd); o.flush()
            val rep = readN(i, 10)
            if (rep[1] != 0.toByte()) return "FAIL socks-connect rep=${rep[1]}"

            // 2) TLS with SNI telegram.org
            val ctx = SSLContext.getInstance("TLS")
            ctx.init(null, arrayOf<X509TrustManager>(object : X509TrustManager {
                override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) {}
                override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {}
                override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
            }), null)
            val f: SSLSocketFactory = ctx.getSocketFactory()
            ssl = f.createSocket(sock, "telegram.org", 443, true) as SSLSocket
            ssl.tcpNoDelay = true
            val params: SSLParameters = ssl.sslParameters
            params.serverNames = listOf(SNIHostName("telegram.org"))
            ssl.sslParameters = params
            ssl.startHandshake()

            // 3) GET the test image
            val so: OutputStream = ssl.getOutputStream()
            val si: InputStream = ssl.getInputStream()
            val req = "GET $TEST_URL_PATH HTTP/1.1\r\n" +
                "Host: telegram.org\r\n" +
                "User-Agent: NukeraTgTest/1.0\r\n" +
                "Accept: image/webp,*/*\r\n" +
                "Connection: close\r\n\r\n"
            so.write(req.toByteArray()); so.flush()

            // 4) headers + full body
            val hdr = StringBuilder()
            while (true) {
                val c = si.read()
                if (c < 0) return "FAIL eof-in-headers"
                hdr.append(c.toChar())
                if (hdr.endsWith("\r\n\r\n")) break
                if (hdr.length > 8192) return "FAIL huge-headers"
            }
            val status = hdr.toString().substringAfter("HTTP/1.1 ").substringBefore(" ").trim()
            val clen = hdr.toString().split("\r\n").firstOrNull { it.startsWith("Content-Length:", true) }
                ?.substringAfter(":")?.trim()
            val now = System.currentTimeMillis()
            val md = MessageDigest.getInstance("SHA-256")
            var n = 0L
            val buf = ByteArray(65536)
            while (true) {
                val r = si.read(buf)
                if (r < 0) break
                md.update(buf, 0, r)
                n += r
                if (System.currentTimeMillis() - now > 60000) return "FAIL read-timeout bytes=$n"
            }
            val sha = md.digest().joinToString("") { "%02x".format(it) }
            return "http=$status bytes=$n clen=$clen sha256=$sha"
        } catch (e: Exception) {
            return "FAIL ${e::class.simpleName}: ${e.message}"
        } finally {
            try { ssl?.close() } catch (_: Exception) {}
            try { sock?.close() } catch (_: Exception) {}
        }
    }

    private fun readN(i: InputStream, n: Int): ByteArray {
        val b = ByteArray(n)
        var off = 0
        while (off < n) {
            val r = i.read(b, off, n - off)
            if (r < 0) throw IllegalStateException("eof")
            off += r
        }
        return b
    }
}