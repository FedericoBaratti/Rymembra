package io.github.federicobaratti.rymembra

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Process
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession.ControllerInfo
import java.security.MessageDigest

/**
 * Chi può collegarsi alla sessione di Rymembra: sfogliare la libreria, la
 * cronologia e le playlist, cercare, sapere che cosa suona, comandare il
 * lettore, mettere un «mi piace».
 *
 * L'ha deciso chi usa Rymembra: l'app stessa, l'interfaccia del sistema
 * (notifica, schermata di blocco, impostazioni rapide), il Bluetooth
 * attraverso il sistema, Android Auto e il centro multimediale delle auto con
 * Android, l'Assistente Google, e le app a cui chi usa il telefono ha dato
 * l'accesso alle notifiche — l'app di un orologio, un telecomando musicale:
 * l'accesso l'ha concesso lui, a mano, in una pagina delle impostazioni che
 * avverte di che cosa vuol dire. Nessun'altra app: la libreria e quel che si
 * ascolta restano sul telefono (PRIVACY.md).
 *
 * # Come li si riconosce
 *
 * - **L'app stessa**: dallo uid di chi chiama, che lo dà Binder e non si
 *   finge. Sono la `MediaController` di `MainActivity` e quella con cui Media3
 *   tiene la notifica.
 * - **Il sistema**: `ControllerInfo.isTrusted` di Media3, cioè lo uid di
 *   sistema, `MEDIA_CONTENT_CONTROL` o `STATUS_BAR_SERVICE` (l'interfaccia del
 *   sistema, il Bluetooth), o un ascoltatore delle notifiche che chi usa il
 *   telefono ha abilitato (gli orologi, i telecomandi). L'ultimo Media3 lo
 *   decide sul nome del pacchetto, che un controllore Media3 dichiara da sé:
 *   per quelli il nome si pretende dello uid che chiama ([pacchettoVero]). E
 *   per deciderlo Media3 deve vedere il pacchetto: il `<queries>` del
 *   manifesto nomina chi dichiara un ascoltatore delle notifiche, e senza
 *   quella riga `isTrusted` era sempre falso per loro.
 * - **Prima di Android 9** la sessione del sistema non dice chi chiama: lo uid
 *   è ignoto. Lì arriva solo chi ha il gettone della sessione — il sistema, il
 *   Bluetooth, gli ascoltatori delle notifiche, o un controllore già entrato da
 *   qui — e rifiutarlo vorrebbe dire spegnere i comandi del Bluetooth su
 *   Android 8.
 * - **Android Auto** ([ANDROID_AUTO]): per pacchetto, uid e certificato di
 *   Google. È `isTrusted` solo se ha l'accesso alle notifiche, e senza
 *   quell'accesso l'auto non vedrebbe più Rymembra.
 * - **Le auto con Android** ([AUTO_DI_SISTEMA]): il centro multimediale e il
 *   lanciatore, per pacchetto e uid, se firmati con la chiave della
 *   piattaforma, cioè componenti del sistema dell'auto.
 * - **L'Assistente Google** ([ASSISTENTE]): come Android Auto, per pacchetto,
 *   uid e certificato di Google. «Pausa», «avanti», «metti i Queen» detti
 *   all'assistente arrivano da lì; senza l'accesso alle notifiche non è
 *   `isTrusted`, e veniva rifiutato.
 *
 * Il nome del pacchetto da solo non basta: su un telefono senza Android Auto
 * un'app qualunque può chiamarsi `com.google.android.projection.gearhead`, e
 * Media3 1.8 non dice se il nome che ha ricevuto è verificato. La verifica si
 * fa qui: lo uid del pacchetto, che `<queries>` nel manifesto rende visibile, e
 * il certificato.
 */
@OptIn(UnstableApi::class)
object Controllori {
  /** Android Auto, sul telefono. */
  const val ANDROID_AUTO = "com.google.android.projection.gearhead"

  /**
   * Il centro multimediale e il lanciatore di Android Automotive: gli stessi
   * nomi di `MediaSession.isAutomotiveController`.
   */
  val AUTO_DI_SISTEMA = setOf("com.android.car.media", "com.android.car.carlauncher")

  /**
   * Gli SHA-256 dei certificati di rilascio con cui Google firma Android Auto:
   * quello di sempre e quello ruotato. Dall'elenco dei chiamanti ammessi di
   * UAMP, l'app d'esempio di Google per i lettori (`allowed_media_browser_callers.xml`,
   * «Android Auto prod release key» e «rotated prod release key»).
   */
  private val CERTIFICATI_ANDROID_AUTO = listOf(
    "fdb00c43dbde8b51cb312aa81d3b5fa17713adb94b28f598d77f8eb89daceedf",
    "1ca8dcc0bed3cbd872d2cb791200c0292ca9975768a82d676b8b424fb65b5295",
  )

  /**
   * L'Assistente Google, col certificato di rilascio con cui Google lo firma:
   * l'app Google sul telefono, e l'Assistente delle auto con Android. Dallo
   * stesso elenco di UAMP («Google» e «Google Assistant on Android Automotive
   * OS»), solo le chiavi di rilascio: quelle di debug firmano copie di prova
   * che su un telefono vero non ci sono.
   */
  val ASSISTENTE = mapOf(
    "com.google.android.googlequicksearchbox" to
      listOf("f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83"),
    "com.google.android.carassistant" to
      listOf("74b6fbf710e8d90d44d340125889b42306a62c4379d0e5a66220e3a68abf90e2"),
  )

  /** Il controllore può entrare. Non lancia: un dubbio vale «no». */
  fun fidato(contesto: Context, controllore: ControllerInfo): Boolean =
    try {
      decidi(contesto.packageManager, controllore)
    } catch (e: Exception) {
      false
    }

  private fun decidi(pm: PackageManager, controllore: ControllerInfo): Boolean {
    val uid = controllore.uid
    if (uid == Process.myUid()) return true
    if (uid < 0) return Build.VERSION.SDK_INT < Build.VERSION_CODES.P
    if (!pacchettoVero(pm, controllore)) return false
    if (controllore.isTrusted) return true
    val pacchetto = controllore.packageName
    return when (pacchetto) {
      ANDROID_AUTO ->
        pm.getPackageUid(pacchetto, 0) == uid && firmatoCon(pm, pacchetto, CERTIFICATI_ANDROID_AUTO)
      in ASSISTENTE.keys ->
        pm.getPackageUid(pacchetto, 0) == uid && firmatoCon(pm, pacchetto, ASSISTENTE.getValue(pacchetto))
      in AUTO_DI_SISTEMA ->
        pm.getPackageUid(pacchetto, 0) == uid &&
          pm.checkSignatures("android", pacchetto) == PackageManager.SIGNATURE_MATCH
      else -> false
    }
  }

  /**
   * Il pacchetto è davvero di chi chiama. Per chi passa dalla sessione del
   * sistema, o da un `MediaBrowser`, il nome lo dà Android; un controllore
   * Media3 lo dichiara da sé, e allora lo si cerca fra i pacchetti del suo uid.
   * Un'app che si collega a un servizio di Rymembra per Android è visibile a
   * Rymembra, quindi la domanda sul suo uid ha una risposta.
   */
  private fun pacchettoVero(pm: PackageManager, controllore: ControllerInfo): Boolean =
    controllore.controllerVersion == ControllerInfo.LEGACY_CONTROLLER_VERSION ||
      pm.getPackagesForUid(controllore.uid)?.contains(controllore.packageName) == true

  /** Il pacchetto è firmato con uno di questi `certificati` (SHA-256). */
  @Suppress("DEPRECATION")
  private fun firmatoCon(pm: PackageManager, pacchetto: String, certificati: List<String>): Boolean {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      // Anche i certificati passati della rotazione, come vuole la rotazione.
      return certificati.any {
        pm.hasSigningCertificate(pacchetto, daEsadecimale(it), PackageManager.CERT_INPUT_SHA256)
      }
    }
    // Prima di Android 9 niente rotazione: una firma sola, e la si confronta.
    val firme = pm.getPackageInfo(pacchetto, PackageManager.GET_SIGNATURES).signatures.orEmpty()
    val impronta = firme.singleOrNull()?.let {
      inEsadecimale(MessageDigest.getInstance("SHA-256").digest(it.toByteArray()))
    }
    return impronta != null && impronta in certificati
  }

  private fun daEsadecimale(testo: String): ByteArray =
    ByteArray(testo.length / 2) { testo.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

  private fun inEsadecimale(dati: ByteArray): String = dati.joinToString("") { "%02x".format(it) }
}
