package io.github.federicobaratti.rymembra

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * I segreti di Rymembra, cifrati con una chiave del Keystore.
 *
 * La chiave — AES a 256 bit, in modalità GCM — la genera il Keystore e non
 * esce mai da lì: su quasi tutti i telefoni la custodisce l'hardware. Nelle
 * preferenze private dell'app resta soltanto il testo cifrato, con il suo
 * vettore iniziale davanti. La chiave della voce entra nella cifratura come
 * dato associato: un testo cifrato copiato sotto un'altra voce non si apre.
 *
 * Le preferenze sono escluse dal backup di Android (`estrazione.xml`): anche se
 * finissero su un altro telefono, lì la chiave non ci sarebbe.
 */
object Segreti {
  private const val ALIAS = "rymembra.segreti"
  private const val PREFERENZE = "rymembra.segreti"
  private const val TRASFORMAZIONE = "AES/GCM/NoPadding"
  private const val LUNGHEZZA_IV = 12
  private const val LUNGHEZZA_ETICHETTA = 128

  private fun chiave(): SecretKey {
    val archivio = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (archivio.getKey(ALIAS, null) as? SecretKey)?.let { return it }
    val generatore = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generatore.init(
      KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build()
    )
    return generatore.generateKey()
  }

  private fun preferenze(contesto: Context) =
    contesto.getSharedPreferences(PREFERENZE, Context.MODE_PRIVATE)

  @Synchronized
  fun leggi(contesto: Context, voce: String): String? {
    val conservato = preferenze(contesto).getString(voce, null) ?: return null
    val byte = Base64.decode(conservato, Base64.NO_WRAP)
    require(byte.size > LUNGHEZZA_IV) { "la voce $voce è troncata" }
    val cifrario = Cipher.getInstance(TRASFORMAZIONE)
    cifrario.init(
      Cipher.DECRYPT_MODE,
      chiave(),
      GCMParameterSpec(LUNGHEZZA_ETICHETTA, byte, 0, LUNGHEZZA_IV)
    )
    cifrario.updateAAD(voce.toByteArray(Charsets.UTF_8))
    val chiaro = cifrario.doFinal(byte, LUNGHEZZA_IV, byte.size - LUNGHEZZA_IV)
    return String(chiaro, Charsets.UTF_8)
  }

  @Synchronized
  fun scrivi(contesto: Context, voce: String, valore: String) {
    val cifrario = Cipher.getInstance(TRASFORMAZIONE)
    cifrario.init(Cipher.ENCRYPT_MODE, chiave())
    cifrario.updateAAD(voce.toByteArray(Charsets.UTF_8))
    val cifrato = cifrario.doFinal(valore.toByteArray(Charsets.UTF_8))
    val iv = cifrario.iv
    check(iv.size == LUNGHEZZA_IV) { "vettore iniziale di ${iv.size} byte" }
    val testo = Base64.encodeToString(iv + cifrato, Base64.NO_WRAP)
    // `commit` e non `apply`: chi scrive un token deve sapere che è su disco
    // prima di dire «collegato».
    check(preferenze(contesto).edit().putString(voce, testo).commit()) { "le preferenze non si scrivono" }
  }

  @Synchronized
  fun cancella(contesto: Context, voce: String) {
    check(preferenze(contesto).edit().remove(voce).commit()) { "le preferenze non si scrivono" }
  }
}
