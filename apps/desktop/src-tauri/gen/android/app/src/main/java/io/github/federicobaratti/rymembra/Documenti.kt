package io.github.federicobaratti.rymembra

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import org.json.JSONObject
import java.io.File

/**
 * Le cartelle e i documenti, attraverso i selettori di sistema.
 *
 * I selettori danno URI `content://`. Una cartella si traduce nel suo percorso
 * vero, perché la scansione la cammina con le funzioni di sempre; un documento
 * si copia nella cache (aprire) o si riempie da un file della cache (salvare),
 * perché il nucleo legge e scrive file. Vedi `src-tauri/src/telefono.rs`.
 */
object Documenti {
  /** L'autorità dei documenti della memoria del telefono e delle schede SD. */
  private const val MEMORIA = "com.android.externalstorage.documents"

  /**
   * `null` se chi guarda ha annullato; `{"percorso": …}` per una cartella
   * della memoria del telefono o di una scheda SD; `{"fuori": <fornitore>}` per
   * una che non sta lì — «Download», un'app cloud che offre cartelle — e che
   * quindi non ha un percorso da camminare. Non un'eccezione: è una scelta che
   * il selettore permette, e Rust la dice con una frase sua invece che come un
   * guasto (vedi `telefono::telefono_scegli_cartella`).
   */
  fun scegliCartella(attivita: MainActivity): JSONObject? {
    val risultato = attivita.aspettaRisultato(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE))
    if (risultato.resultCode != Activity.RESULT_OK) return null
    val albero = risultato.data?.data ?: return null
    val percorso = percorsoDi(albero) ?: return JSONObject().put("fuori", albero.authority ?: "")
    return JSONObject().put("percorso", percorso)
  }

  /**
   * `content://…externalstorage.documents/tree/primary:Music/Rock` diventa
   * `/storage/emulated/0/Music/Rock`; un volume che non è `primary` — una
   * scheda SD, `1A2B-3C4D` — diventa `/storage/1A2B-3C4D/…`. `null` per una
   * cartella di un altro fornitore, che un percorso non ce l'ha.
   */
  fun percorsoDi(albero: Uri): String? {
    if (albero.authority != MEMORIA) return null
    val id = DocumentsContract.getTreeDocumentId(albero)
    val volume = id.substringBefore(':')
    val resto = id.substringAfter(':', "").trim('/')
    val base =
      if (volume.equals("primary", ignoreCase = true)) Environment.getExternalStorageDirectory().absolutePath
      else "/storage/$volume"
    return if (resto.isEmpty()) base else "$base/$resto"
  }

  fun apri(attivita: MainActivity, tipi: List<String>): String? {
    val intento = Intent(Intent.ACTION_OPEN_DOCUMENT)
      .addCategory(Intent.CATEGORY_OPENABLE)
      .setType("*/*")
    if (tipi.isNotEmpty()) intento.putExtra(Intent.EXTRA_MIME_TYPES, tipi.toTypedArray())
    val risultato = attivita.aspettaRisultato(intento)
    if (risultato.resultCode != Activity.RESULT_OK) return null
    val uri = risultato.data?.data ?: return null
    // Solo il nome, mai un percorso: lo dà il fornitore del documento, e una
    // barra dentro porterebbe la copia fuori dalla cache.
    val nome = File(nomeDi(attivita, uri) ?: "documento").name
    val cartella = File(attivita.cacheDir, "aperti").apply { mkdirs() }
    val copia = File(cartella, nome)
    val ingresso = attivita.contentResolver.openInputStream(uri) ?: error("il documento non si apre")
    ingresso.use { dentro -> copia.outputStream().use { fuori -> dentro.copyTo(fuori) } }
    return copia.absolutePath
  }

  fun salva(attivita: MainActivity, nome: String, tipo: String, sorgente: String): Boolean {
    val intento = Intent(Intent.ACTION_CREATE_DOCUMENT)
      .addCategory(Intent.CATEGORY_OPENABLE)
      .setType(tipo.ifBlank { "application/octet-stream" })
      .putExtra(Intent.EXTRA_TITLE, nome)
    val risultato = attivita.aspettaRisultato(intento)
    if (risultato.resultCode != Activity.RESULT_OK) return false
    val uri = risultato.data?.data ?: return false
    val uscita = attivita.contentResolver.openOutputStream(uri, "wt") ?: error("il documento non si scrive")
    uscita.use { fuori -> File(sorgente).inputStream().use { dentro -> dentro.copyTo(fuori) } }
    return true
  }

  private fun nomeDi(contesto: Context, uri: Uri): String? =
    contesto.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursore ->
      if (cursore.moveToFirst()) cursore.getString(0) else null
    }
}
