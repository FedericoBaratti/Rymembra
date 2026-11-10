package io.github.federicobaratti.rymembra

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.File
import java.io.FileNotFoundException

/**
 * Le copertine, per chi non è Rymembra: l'auto, la schermata di blocco, la
 * scheda di ripresa del sistema.
 *
 * Le copertine stanno nella cartella privata dell'app, e un altro processo un
 * `file://` di lì non lo apre (è l'`EACCES` che SystemUI scriveva nel
 * registro). Questo fornitore le serve in sola lettura, a chiunque le chieda,
 * e **solo** loro: il percorso deve essere `xx/<impronta>[.m|.t].jpg` dentro
 * la cartella delle copertine, come lo scrive `CoverStore` in Rust, e un
 * percorso che esce di lì — `..`, un collegamento — non si apre. Le copertine
 * non sono un segreto, ma il database e i segreti cifrati stanno due cartelle
 * più in là.
 *
 * La cartella la dice Rust quando parte (`sessione.pronta`), e si ricorda: il
 * sistema può chiedere una copertina dopo un riavvio, prima che il nucleo ci
 * sia.
 */
class Copertine : ContentProvider() {
  override fun onCreate(): Boolean = true

  override fun getType(uri: Uri): String = "image/jpeg"

  override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
    if (mode != "r") throw SecurityException("le copertine si leggono e basta")
    val radice = cartella(context ?: throw FileNotFoundException("nessun contesto"))
      ?: throw FileNotFoundException("la cartella delle copertine non è ancora nota")
    val parti = uri.pathSegments
    if (parti.size != 2 || !SOTTOCARTELLA.matches(parti[0]) || !NOME.matches(parti[1])) {
      throw FileNotFoundException("non è una copertina: $uri")
    }
    val file = File(File(radice, parti[0]), parti[1]).canonicalFile
    if (!file.path.startsWith(radice.canonicalPath + File.separator)) {
      throw FileNotFoundException("fuori dalle copertine: $uri")
    }
    return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
  }

  override fun query(
    uri: Uri,
    projection: Array<out String>?,
    selection: String?,
    selectionArgs: Array<out String>?,
    sortOrder: String?,
  ): Cursor? = null

  override fun insert(uri: Uri, values: ContentValues?): Uri? = null

  override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?) = 0

  override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?) = 0

  companion object {
    private const val FILE = "copertine"
    private val SOTTOCARTELLA = Regex("[0-9A-Za-z_]{1,8}")
    private val NOME = Regex("[0-9A-Za-z_]{1,128}(\\.[mt])?\\.jpg")

    @Volatile private var radice: File? = null

    private fun autorita() = "${BuildConfig.APPLICATION_ID}.copertine"

    /** L'indirizzo di una copertina, dal suo percorso dentro la cartella. */
    fun indirizzo(relativa: String): Uri =
      Uri.Builder().scheme("content").authority(autorita()).appendEncodedPath(relativa).build()

    /** La cartella delle copertine, come l'ha detta Rust. */
    fun ricorda(contesto: Context, percorso: String) {
      radice = File(percorso)
      contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putString("cartella", percorso).apply()
    }

    private fun cartella(contesto: Context): File? =
      radice ?: contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        .getString("cartella", null)
        ?.let(::File)
        ?.also { radice = it }
  }
}
