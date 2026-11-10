package io.github.federicobaratti.rymembra

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File

/**
 * La consegna di un aggiornamento all'installatore di Android.
 *
 * Rymembra scarica l'APK nella sua cache (`src/mobile/aggiornamenti.rs`) e lo
 * passa qui. Prima di consegnarlo lo si apre, e si pretende che sia **questa**
 * app: lo stesso pacchetto, firmato con lo stesso certificato della Rymembra
 * installata ([verifica]). L'installatore di Android controlla la firma da sé,
 * ma solo sopra un pacchetto che c'è già: un APK con un altro nome di
 * pacchetto per lui è un'app nuova, e la propone da installare — col nome e
 * l'icona che l'APK si è dato — a chi ha appena chiesto un aggiornamento. Un
 * APK che non passa si cancella, e all'installatore non arriva.
 *
 * La prima volta Android vuole che si permetta a Rymembra di installare app:
 * allora si apre la pagina delle impostazioni dove lo si concede, e il secondo
 * «Aggiorna» trova l'APK già sceso.
 */
object Installatore {
  /** La cartella della cache dove Rust scarica l'APK. */
  private const val CARTELLA = "aggiornamenti"

  /** `installatore` o `permesso`: vedi `sistema::Installazione` in Rust. */
  fun installa(attivita: MainActivity, percorso: String): String {
    val file = apk(attivita, percorso)
    // Prima del permesso: un APK che non è Rymembra non vale la domanda.
    verifica(attivita, file)
    if (!attivita.packageManager.canRequestPackageInstalls()) {
      attivita.runOnUiThread {
        attivita.startActivity(
          Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${attivita.packageName}"))
        )
      }
      return "permesso"
    }
    val indirizzo = FileProvider.getUriForFile(attivita, "${attivita.packageName}.fileprovider", file)
    val intento = Intent(Intent.ACTION_VIEW)
      .setDataAndType(indirizzo, TIPO_APK)
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    intento.component = installatoreDiSistema(attivita.packageManager, intento)
    attivita.runOnUiThread { attivita.startActivity(intento) }
    return "installatore"
  }

  private const val TIPO_APK = "application/vnd.android.package-archive"

  /**
   * L'installatore del sistema, per nome. Con l'intento implicito di prima
   * ogni app che dichiara di aprire gli APK compariva fra le scelte, e
   * riceveva il permesso di leggerlo: un finto installatore in mezzo ai veri.
   * Qui si guardano solo le app di sistema che aprono un APK, e fra loro si
   * preferisce quella che può installare pacchetti (l'installatore vero, non
   * un gestore file di sistema che l'APK lo mostra soltanto). Senza nessuna
   * app di sistema che lo apra è un errore, non l'intento implicito: vedere
   * la richiesta serve il `<queries>` del manifesto.
   */
  @Suppress("DEPRECATION")
  private fun installatoreDiSistema(pm: PackageManager, intento: Intent): ComponentName {
    val bandiere = PackageManager.MATCH_SYSTEM_ONLY or PackageManager.MATCH_DEFAULT_ONLY
    val candidati =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        pm.queryIntentActivities(intento, PackageManager.ResolveInfoFlags.of(bandiere.toLong()))
      } else {
        pm.queryIntentActivities(intento, bandiere)
      }
    val scelto = candidati.firstOrNull {
      pm.checkPermission(Manifest.permission.INSTALL_PACKAGES, it.activityInfo.packageName) ==
        PackageManager.PERMISSION_GRANTED
    } ?: candidati.firstOrNull() ?: error("sul telefono non c'è l'installatore di sistema")
    return ComponentName(scelto.activityInfo.packageName, scelto.activityInfo.name)
  }

  /**
   * Il file da installare: un APK della cartella degli aggiornamenti, e nessun
   * altro. È anche il solo file che [verifica] può cancellare.
   */
  private fun apk(attivita: MainActivity, percorso: String): File {
    val cartella = File(attivita.cacheDir, CARTELLA).canonicalFile
    val file = File(percorso).canonicalFile
    check(file.parentFile == cartella) { "l'APK non sta nella cartella degli aggiornamenti: $percorso" }
    check(file.isFile) { "l'APK non c'è: $percorso" }
    return file
  }

  /**
   * L'APK è questa app, firmata come questa app; se no si cancella, e
   * l'eccezione arriva a Rust come errore dell'aggiornamento. Chiusa per
   * difetto: un archivio che non si legge, o che non porta firme, vale come
   * un archivio sbagliato.
   */
  private fun verifica(attivita: MainActivity, file: File) {
    fun rifiuta(perche: String): Nothing {
      file.delete()
      throw SecurityException("l'aggiornamento scaricato non è questa Rymembra: $perche")
    }
    val pm = attivita.packageManager
    val archivio = leggi(pm, file.path) ?: rifiuta("l'APK non si legge")
    if (archivio.packageName != attivita.packageName) {
      rifiuta("è il pacchetto ${archivio.packageName}")
    }
    if (!stessaFirma(archivio, installata(pm, attivita.packageName))) {
      rifiuta("è firmato con un'altra chiave")
    }
  }

  /**
   * Le firme, chieste in modo che arrivino davvero.
   *
   * Da Android 9 la forma nuova è `GET_SIGNING_CERTIFICATES`; ma per un
   * **archivio** Android 9, 10 e 13 raccolgono i certificati solo se c'è anche
   * `GET_SIGNATURES` (`PackageManager.getPackageArchiveInfo` di AOSP: `if
   * ((flags & GET_SIGNATURES) != 0) collectCertificates(…)`). Con la sola forma
   * nuova `signingInfo` torna nullo, e [verifica], chiusa per difetto,
   * rifiuterebbe ogni aggiornamento. `GET_SIGNATURES` non va tolto.
   *
   * Fino ad Android 8.1 basta `GET_SIGNATURES`: `collectCertificates` legge le
   * firme (v1 e v2) dal file stesso. `applicationInfo.sourceDir` e
   * `publicSourceDir` servono solo a caricare l'icona e il nome di un archivio,
   * non le sue firme.
   */
  @Suppress("DEPRECATION")
  private val FIRME: Int =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      PackageManager.GET_SIGNING_CERTIFICATES or PackageManager.GET_SIGNATURES
    } else {
      PackageManager.GET_SIGNATURES
    }

  @Suppress("DEPRECATION")
  private fun leggi(pm: PackageManager, percorso: String): PackageInfo? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      pm.getPackageArchiveInfo(percorso, PackageManager.PackageInfoFlags.of(FIRME.toLong()))
    } else {
      pm.getPackageArchiveInfo(percorso, FIRME)
    }

  @Suppress("DEPRECATION")
  private fun installata(pm: PackageManager, pacchetto: String): PackageInfo =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      pm.getPackageInfo(pacchetto, PackageManager.PackageInfoFlags.of(FIRME.toLong()))
    } else {
      pm.getPackageInfo(pacchetto, FIRME)
    }

  /**
   * Lo stesso firmatario. Con un firmatario solo vale anche quello nuovo che
   * dimostra di discendere da questo (la rotazione della chiave dello schema
   * v3): è la regola con cui Android accetta l'aggiornamento, e rifiutarlo qui
   * vorrebbe dire bloccarne uno legittimo.
   */
  @Suppress("DEPRECATION")
  private fun stessaFirma(archivio: PackageInfo, installata: PackageInfo): Boolean {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      val sua = archivio.signingInfo ?: return false
      val mia = installata.signingInfo ?: return false
      if (sua.hasMultipleSigners() || mia.hasMultipleSigners()) {
        val suoi = sua.apkContentsSigners?.toSet().orEmpty()
        return suoi.isNotEmpty() && suoi == mia.apkContentsSigners?.toSet().orEmpty()
      }
      val attuale = mia.signingCertificateHistory?.lastOrNull() ?: return false
      return sua.signingCertificateHistory?.contains(attuale) == true
    }
    val suoi = archivio.signatures?.toSet().orEmpty()
    return suoi.isNotEmpty() && suoi == installata.signatures?.toSet().orEmpty()
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
