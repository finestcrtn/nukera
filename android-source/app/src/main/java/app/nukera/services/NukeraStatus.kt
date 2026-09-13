package app.nukera.services

import app.nukera.data.AppStatus
import app.nukera.data.Mode

var appStatus = AppStatus.Halted to Mode.VPN
    private set

fun setStatus(status: AppStatus, mode: Mode) {
    appStatus = status to mode
}
