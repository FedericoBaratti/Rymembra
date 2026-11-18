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
 * e **solo** loro. Le radici sono due, e ogni percorso ha una forma sola:
 *
 * - **Lo store di questo dispositivo**: `xx/<impronta>[.m|.t].jpg`, come lo
 *   scrive `CoverStore` in Rust.
 * - **La cache delle copertine degli altri dispositivi collegati** (un brano
 *   che suona dal PC): `lontane/<dispositivo>/<impronta>.jpg`, con il
 *   dispositivo di 64 cifre esadecimali minuscole e l'impronta di 2-128. Qui
 *   `lontane` non è una sottocartella: è il nome della seconda radice, e il
 *   resto sta dentro di lei, come lo scrive `lontano::sfoglia` in Rust.
 *
 * Un percorso che esce dalla sua radice — `..`, un collegamento — non si apre
 * (il controllo è sul percorso canonico, e la radice si canonicalizza ogni
 * volta: `/data/user/0` e `/data/data` sono lo stesso posto con due nomi). Le
 * copertine non sono un segreto, ma il database e i segreti cifrati stanno due
 * cartelle più in là.
 *
 * Le radici le dice Rust quando parte (`sessione.pronta`), e si ricordano: il
 * sistema può chiedere una copertina dopo un riavvio, prima che il nucleo ci
 * sia. Se Rust non ha una cache per le copertine degli altri, la seconda
 * radice si toglie, e i percorsi `lontane/…` non si aprono.
 */
class Copertine : ContentProvider() {
  override fun onCreate(): Boolean = true

  override fun getType(uri: Uri): String = "image/jpeg"

  override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
    if (mode != "r") throw SecurityException("le copertine si leggono e basta")
    val contesto = context ?: throw FileNotFoundException("nessun contesto")
    val parti = uri.pathSegments
    val (radice, file) = when {
      parti.size == 2 && SOTTOCARTELLA.matches(parti[0]) && NOME.matches(parti[1]) -> {
        val radice = cartella(contesto)
          ?: throw FileNotFoundException("la cartella delle copertine non è ancora nota")
        radice to File(File(radice, parti[0]), parti[1])
      }
      parti.size == 3 && parti[0] == LONTANE && DISPOSITIVO.matches(parti[1]) &&
        IMPRONTA_LONTANA.matches(parti[2]) -> {
        val radice = cartellaLontane(contesto)
          ?: throw FileNotFoundException("la cartella delle copertine lontane non è nota")
        radice to File(File(radice, parti[1]), parti[2])
      }
      else -> throw FileNotFoundException("non è una copertina: $uri")
    }
    val canonico = file.canonicalFile
    if (!canonico.path.startsWith(radice.canonicalPath + File.separator)) {
      throw FileNotFoundException("fuori dalle copertine: $uri")
    }
    return ParcelFileDescriptor.open(canonico, ParcelFileDescriptor.MODE_READ_ONLY)
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

    /** Il nome con cui un percorso dice «nella seconda radice». */
    private const val LONTANE = "lontane"
    private val SOTTOCARTELLA = Regex("[0-9A-Za-z_]{1,8}")
    private val NOME = Regex("[0-9A-Za-z_]{1,128}(\\.[mt])?\\.jpg")
    private val DISPOSITIVO = Regex("[0-9a-f]{64}")
    private val IMPRONTA_LONTANA = Regex("[0-9a-f]{2,128}\\.jpg")

    @Volatile private var radice: File? = null
    @Volatile private var radiceLontane: File? = null

    private fun autorita() = "${BuildConfig.APPLICATION_ID}.copertine"

    /** L'indirizzo di una copertina, dal suo percorso dentro la cartella. */
    fun indirizzo(relativa: String): Uri =
      Uri.Builder().scheme("content").authority(autorita()).appendEncodedPath(relativa).build()

    /**
     * Le cartelle delle copertine, come le ha dette Rust: lo store di questo
     * dispositivo, e la cache degli altri (`null` se Rust non ne ha una).
     */
    fun ricorda(contesto: Context, percorso: String, lontane: String?) {
      radice = File(percorso)
      radiceLontane = lontane?.let(::File)
      contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
        .putString("cartella", percorso)
        // `null` toglie la voce: una cache che non c'è più non resta ricordata.
        .putString("lontane", lontane)
        .apply()
    }

    private fun cartella(contesto: Context): File? =
      radice ?: contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        .getString("cartella", null)
        ?.let(::File)
        ?.also { radice = it }

    private fun cartellaLontane(contesto: Context): File? =
      radiceLontane ?: contesto.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        .getString("lontane", null)
        ?.let(::File)
        ?.also { radiceLontane = it }
  }
}
