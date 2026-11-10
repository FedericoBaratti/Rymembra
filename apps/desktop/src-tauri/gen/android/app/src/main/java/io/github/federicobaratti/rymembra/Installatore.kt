package io.github.federicobaratti.rymembra

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File

/**
 * La consegna di un aggiornamento all'installatore di Android.
 *
 * Rymembra scarica l'APK nella sua cache (`src/mobile/aggiornamenti.rs`) e lo
 * passa qui; da qui in poi è del sistema: l'installatore chiede conferma, e
 * installa solo se l'APK è firmato con la stessa chiave della Rymembra che c'è
 * già. È quella verifica, e non una nostra, a garantire che sopra Rymembra
 * finisca solo Rymembra.
 *
 * La prima volta Android vuole che si permetta a Rymembra di installare app:
 * allora si apre la pagina delle impostazioni dove lo si concede, e il secondo
 * «Aggiorna» trova l'APK già sceso.
 */
object Installatore {
  /** `installatore` o `permesso`: vedi `sistema::Installazione` in Rust. */
  fun installa(attivita: MainActivity, percorso: String): String {
    if (!attivita.packageManager.canRequestPackageInstalls()) {
      attivita.runOnUiThread {
        attivita.startActivity(
          Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${attivita.packageName}"))
        )
      }
      return "permesso"
    }
    val file = File(percorso)
    check(file.isFile) { "l'APK non c'è: $percorso" }
    val indirizzo = FileProvider.getUriForFile(attivita, "${attivita.packageName}.fileprovider", file)
    attivita.runOnUiThread {
      attivita.startActivity(
        Intent(Intent.ACTION_VIEW)
          .setDataAndType(indirizzo, "application/vnd.android.package-archive")
          .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      )
    }
    return "installatore"
  }
}

/**
 * Il foglio di condivisione di Android, per un file della cache: il diario da
 * mandare con una segnalazione, che sul telefono non ha una cartella da aprire.
 */
object Condividi {
  fun file(attivita: MainActivity, percorso: String, tipo: String, titolo: String) {
    val file = File(percorso)
    check(file.isFile) { "il file da condividere non c'è: $percorso" }
    val indirizzo = FileProvider.getUriForFile(attivita, "${attivita.packageName}.fileprovider", file)
    val intento = Intent(Intent.ACTION_SEND)
      .setType(tipo)
      .putExtra(Intent.EXTRA_STREAM, indirizzo)
      .putExtra(Intent.EXTRA_SUBJECT, file.name)
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    attivita.runOnUiThread { attivita.startActivity(Intent.createChooser(intento, titolo)) }
  }
}
