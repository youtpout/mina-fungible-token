package com.lumina.minatokennative

import android.app.Activity
import android.os.Bundle
import android.text.InputType
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.ProgressBar
import android.widget.TextView
import android.view.View
import org.json.JSONObject
import java.util.concurrent.Executors

class MainActivity : Activity() {
    private val executor = Executors.newSingleThreadExecutor()

    private external fun nativeBackendInfo(): String
    private external fun nativeTransfer(requestJson: String): String
    private external fun nativeTokenBalance(requestJson: String): String

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        val privateKey = findViewById<EditText>(R.id.privateKey)
        privateKey.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        val receiver = findViewById<EditText>(R.id.receiver)
        val amount = findViewById<EditText>(R.id.amount)
        val tokenAddress = findViewById<EditText>(R.id.tokenAddress)
        val graphqlUrl = findViewById<EditText>(R.id.graphqlUrl)
        val fundReceiver = findViewById<CheckBox>(R.id.fundReceiver)
        val status = findViewById<TextView>(R.id.backendStatus)
        val result = findViewById<TextView>(R.id.result)
        val timings = findViewById<TextView>(R.id.timings)
        val progress = findViewById<ProgressBar>(R.id.progress)
        val send = findViewById<Button>(R.id.sendButton)
        val checkBalance = findViewById<Button>(R.id.checkBalanceButton)
        val tokenBalance = findViewById<TextView>(R.id.tokenBalance)

        executor.execute {
            val info = nativeBackendInfo()
            runOnUiThread {
                status.text = "Native Rust backend loaded"
                result.text = info
                progress.visibility = View.GONE
                send.isEnabled = true
                checkBalance.isEnabled = true
            }
        }

        checkBalance.setOnClickListener {
            send.isEnabled = false
            checkBalance.isEnabled = false
            progress.visibility = View.VISIBLE
            status.text = "Loading the selected address token balance…"
            val request = JSONObject()
                .put("address", receiver.text.toString())
                .put("tokenAddress", tokenAddress.text.toString())
                .put("graphqlUrl", graphqlUrl.text.toString())
                .toString()

            executor.execute {
                val response = nativeTokenBalance(request)
                runOnUiThread {
                    tokenBalance.text = runCatching {
                        val json = JSONObject(response)
                        if (json.optString("status") == "ok") {
                            "Token balance: ${json.optString("balance")} smallest units"
                        } else {
                            "Token balance unavailable: ${json.optString("message")}"
                        }
                    }.getOrElse { "Token balance unavailable: native backend error" }
                    progress.visibility = View.GONE
                    status.text = "Balance check completed"
                    send.isEnabled = true
                    checkBalance.isEnabled = true
                }
            }
        }

        send.setOnClickListener {
            send.isEnabled = false
            checkBalance.isEnabled = false
            progress.visibility = View.VISIBLE
            status.text = "Building and proving with native Rust…"
            result.text = "Preparing the transaction…"
            val request = JSONObject()
                .put("senderPrivateKey", privateKey.text.toString())
                .put("receiver", receiver.text.toString())
                .put("amount", amount.text.toString())
                .put("tokenAddress", tokenAddress.text.toString())
                .put("graphqlUrl", graphqlUrl.text.toString())
                .put("fundReceiver", fundReceiver.isChecked)
                .toString()

            executor.execute {
                val response = nativeTransfer(request)
                runOnUiThread {
                    privateKey.text.clear()
                    result.text = response
                    timings.text = formatTimings(response)
                    progress.visibility = View.GONE
                    status.text = "Operation completed"
                    send.isEnabled = true
                    checkBalance.isEnabled = true
                }
            }
        }
    }

    override fun onDestroy() {
        executor.shutdownNow()
        super.onDestroy()
    }

    private fun formatTimings(response: String): String = try {
        val timings = JSONObject(response).optJSONObject("timingsMs")
        fun value(name: String): String = if (timings?.isNull(name) == false) {
            "${timings.optLong(name)} ms"
        } else {
            "—"
        }
        "Compile: ${value("compile")}\n" +
            "Proving: ${value("proving")}\n" +
            "Signature: ${value("signature")}\n" +
            "Submit: ${value("submit")}\n" +
            "Total: ${value("total")}"
    } catch (_: Exception) {
        "Timings unavailable"
    }

    companion object {
        init {
            System.loadLibrary("mina_token_mobile")
        }
    }
}
