package app.nukera.data

data class Command(
    var text: String,
    var pinned: Boolean = false,
    var name: String? = null
)