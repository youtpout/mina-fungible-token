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
        // The header progress bar and status scroll out of view during the
        // long proving run, so mirror them right under the send button.
        val sendProgress = findViewById<ProgressBar>(R.id.sendProgress)
        val sendStatus = findViewById<TextView>(R.id.sendStatus)
        val changeKey = findViewById<Button>(R.id.changeKeyButton)

        // The key is kept for repeat transfers rather than cleared, but it is
        // locked once used so it cannot be edited by accident; replacing it is
        // deliberate and starts from an empty field.
        fun lockKey() {
            privateKey.isEnabled = false
            changeKey.visibility = View.VISIBLE
        }

        changeKey.setOnClickListener {
            privateKey.text.clear()
            privateKey.isEnabled = true
            changeKey.visibility = View.GONE
            privateKey.requestFocus()
        }

        fun showStatus(message: String, busy: Boolean) {
            status.text = message
            sendStatus.text = message
            progress.visibility = if (busy) View.VISIBLE else View.GONE
            sendProgress.visibility = if (busy) View.VISIBLE else View.GONE
            sendStatus.visibility = View.VISIBLE
        }

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
            showStatus("Loading the selected address token balance…", busy = true)
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
                    showStatus("Balance check completed", busy = false)
                    send.isEnabled = true
                    checkBalance.isEnabled = true
                }
            }
        }

        send.setOnClickListener {
            send.isEnabled = false
            checkBalance.isEnabled = false
            showStatus("Building and proving with native Rust…", busy = true)
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
                    lockKey()
                    result.text = response
                    timings.text = formatTimings(response)
                    showStatus(transferStatus(response), busy = false)
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

    private fun transferStatus(response: String): String = try {
        val json = JSONObject(response)
        if (json.optString("status") == "sent") {
            "Transfer submitted: ${json.optString("transactionHash")}"
        } else {
            "Transfer failed: ${json.optString("message").take(120)}"
        }
    } catch (_: Exception) {
        "Operation completed"
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
