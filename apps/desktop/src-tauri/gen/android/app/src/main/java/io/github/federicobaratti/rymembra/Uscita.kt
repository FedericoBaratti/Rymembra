package io.github.federicobaratti.rymembra

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import org.json.JSONObject

/**
 * Dove Android manda la musica adesso.
 *
 * Il motore Rust apre il flusso senza dire su quale uscita (`AAudio` col
 * dispositivo «non specificato»), e così suona dove Android decide: le cuffie
 * Bluetooth appena collegate, il cavo, l'altoparlante. Quel che il motore non
 * sa è **come si chiama** quell'uscita, e gli serve per tre cose: accorgersi che
 * è cambiata (e riaprire il flusso sulla nuova), tenere una latenza per uscita
 * — quella delle cuffie Bluetooth non è quella dell'altoparlante — e scriverlo
 * nel diario. Vedi `rymembra-play/src/dispositivi.rs`.
 */
object Uscita {
  private val MUSICA: AudioAttributes = AudioAttributes.Builder()
    .setUsage(AudioAttributes.USAGE_MEDIA)
    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
    .build()

  /** Le uscite Bluetooth, per chi vuole sapere se la radio c'è di mezzo. */
  private val BLUETOOTH = setOf(
    AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
    AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
    AudioDeviceInfo.TYPE_BLE_HEADSET,
    AudioDeviceInfo.TYPE_BLE_SPEAKER,
    AudioDeviceInfo.TYPE_BLE_BROADCAST,
  )

  /**
   * Senza la risposta di Android (prima di Android 13), l'ordine in cui la sua
   * regola di serie sceglie dove mandare la musica: la radio vince sul cavo, il
   * cavo sull'altoparlante.
   */
  private val PRECEDENZA = listOf(
    AudioDeviceInfo.TYPE_BLE_HEADSET,
    AudioDeviceInfo.TYPE_BLE_SPEAKER,
    AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
    AudioDeviceInfo.TYPE_USB_HEADSET,
    AudioDeviceInfo.TYPE_USB_DEVICE,
    AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
    AudioDeviceInfo.TYPE_WIRED_HEADSET,
    AudioDeviceInfo.TYPE_LINE_ANALOG,
    AudioDeviceInfo.TYPE_LINE_DIGITAL,
    AudioDeviceInfo.TYPE_HDMI,
    AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
  )

  /** `{"nome": …, "bluetooth": …}`, oppure `null` se non c'è nessuna uscita. */
  fun adesso(contesto: Context): JSONObject? {
    val audio = contesto.getSystemService(AudioManager::class.java)
    val uscita = (if (Build.VERSION.SDK_INT >= 33) {
      audio.getAudioDevicesForAttributes(MUSICA).firstOrNull()
    } else {
      null
    }) ?: stimata(audio) ?: return null
    return JSONObject()
      .put("nome", nome(contesto, uscita))
      .put("bluetooth", uscita.type in BLUETOOTH)
  }

  private fun stimata(audio: AudioManager): AudioDeviceInfo? {
    val uscite = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
    return PRECEDENZA.firstNotNullOfOrNull { tipo -> uscite.firstOrNull { it.type == tipo } }
  }

  /**
   * Il nome da mostrare. Quelle dentro il telefono si chiamano come il modello
   * del telefono — «A059P» per l'altoparlante e per l'auricolare — e quindi si
   * dicono per quel che sono; le altre col nome che hanno, quello delle cuffie.
   */
  private fun nome(contesto: Context, uscita: AudioDeviceInfo): String {
    val proprio = uscita.productName?.toString()?.trim().orEmpty()
    val perTipo = when (uscita.type) {
      AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> R.string.uscita_altoparlante
      AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> R.string.uscita_auricolare
      AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET -> R.string.uscita_cuffie
      else -> null
    }
    return when {
      perTipo != null -> contesto.getString(perTipo)
      proprio.isNotEmpty() && proprio != Build.MODEL -> proprio
      uscita.type in BLUETOOTH -> contesto.getString(R.string.uscita_bluetooth)
      else -> contesto.getString(R.string.uscita_altra)
    }
  }
}

