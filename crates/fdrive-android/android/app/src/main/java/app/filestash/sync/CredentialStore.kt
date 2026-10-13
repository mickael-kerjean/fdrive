package app.filestash.sync

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

data class Credentials(
    val url: String,
    val token: String,
)

class CredentialStore(context: Context) {
    private val prefs = EncryptedSharedPreferences.create(
        context,
        "credentials",
        MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build(),
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
    )

    fun load(): Credentials? {
        val url = prefs.getString("url", null) ?: return null
        return Credentials(
            url = url,
            token = prefs.getString("token", "") ?: "",
        )
    }

    fun save(credentials: Credentials) {
        prefs.edit()
            .putString("url", credentials.url)
            .putString("token", credentials.token)
            .apply()
    }

    fun clear() {
        prefs.edit().clear().apply()
    }
}
