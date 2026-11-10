package io.github.federicobaratti.rymembra

import android.app.Activity
import android.content.ContentUris
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import androidx.activity.result.IntentSenderRequest

/**
 * Il Cestino di Android: quello di `MediaStore`, da Android 11.
 *
 * Un file nel Cestino non c'è più per le app — `MediaStore` lo rinomina in
 * `.trashed-…` e lo nasconde — e si recupera per trenta giorni dall'app File;
 * poi il sistema lo cancella da sé. È la stessa promessa del Cestino di
 * Windows, e Rymembra la fa solo dove la può mantenere: un file che
 * `MediaStore` non conosce (un `.cue`, una cartella con `.nomedia`, un telefono
 * con Android 10) non ha un Cestino, e Rust lo tratta come un file su una
 * share di rete — cancellazione definitiva, ma solo dopo averla chiesta.
 *
 * Mandarci i file vuole il consenso di chi usa il telefono: il sistema mostra
 * il suo dialogo, una volta per tutti i file del gesto. Chi ha concesso a
 * Rymembra «Gestione dei contenuti multimediali» non lo vede.
 */
object Cestino {
  /** Per ogni percorso, nello stesso ordine, se il Cestino lo può prendere. */
  fun cestinabili(contesto: Context, percorsi: List<String>): List<Boolean> {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return percorsi.map { false }
    val trovati = indirizzi(contesto, percorsi)
    return percorsi.map { trovati.containsKey(it) }
  }

  /**
   * Manda i file nel Cestino, dopo il dialogo del sistema. `false` se chi
   * guarda ha detto di no: allora non si è mosso niente.
   *
   * Aspetta una persona: mai dal filo principale (vedi `Ponte`).
   */
  fun cestina(attivita: MainActivity, percorsi: List<String>): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
      error("il Cestino del sistema c'è da Android 11")
    }
    val trovati = indirizzi(attivita, percorsi)
    val mancanti = percorsi.filterNot { trovati.containsKey(it) }
    check(mancanti.isEmpty()) { "non nell'indice dei file multimediali: ${mancanti.first()}" }
    if (trovati.isEmpty()) return true
    val richiesta = MediaStore.createTrashRequest(attivita.contentResolver, trovati.values.toList(), true)
    val esito = attivita.aspettaIntentSender(IntentSenderRequest.Builder(richiesta.intentSender).build())
    return esito.resultCode == Activity.RESULT_OK
  }

  /**
   * L'indirizzo in `MediaStore` di ogni percorso che vi compare, nella
   * collezione del suo genere: il Cestino accetta solo audio, immagini, video
   * e scaricati.
   */
  private fun indirizzi(contesto: Context, percorsi: List<String>): Map<String, Uri> {
    val trovati = mutableMapOf<String, Uri>()
    val file = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL)
    val colonne = arrayOf(
      MediaStore.Files.FileColumns._ID,
      MediaStore.Files.FileColumns.DATA,
      MediaStore.Files.FileColumns.MEDIA_TYPE,
    )
    // A blocchi: una domanda con migliaia di `?` supera il tetto di SQLite.
    for (blocco in percorsi.distinct().chunked(BLOCCO)) {
      val domanda = "${MediaStore.Files.FileColumns.DATA} IN (${blocco.joinToString(",") { "?" }})"
      contesto.contentResolver.query(file, colonne, domanda, blocco.toTypedArray(), null)?.use { righe ->
        while (righe.moveToNext()) {
          val id = righe.getLong(0)
          val percorso = righe.getString(1) ?: continue
          val collezione = when (righe.getInt(2)) {
            MediaStore.Files.FileColumns.MEDIA_TYPE_AUDIO -> MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE -> MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO -> MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
            else -> null
          } ?: continue
          trovati[percorso] = ContentUris.withAppendedId(collezione, id)
        }
      }
    }
    return trovati
  }

  private const val BLOCCO = 200
}
