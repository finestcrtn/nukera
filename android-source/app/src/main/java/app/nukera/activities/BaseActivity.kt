package app.nukera.activities

import android.os.Bundle
import androidx.appcompat.app.AppCompatDelegate
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import app.nukera.R
import app.nukera.utility.SettingsUtils
import app.nukera.utility.getPreferences
import app.nukera.utility.getStringNotNull

abstract class BaseActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)

        val prefs = getPreferences()
        val lang = prefs.getStringNotNull("language", "system")
        SettingsUtils.setLang(lang)

        super.onCreate(savedInstanceState)
    }

    protected fun setupToolbar() {
        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
    }

}
