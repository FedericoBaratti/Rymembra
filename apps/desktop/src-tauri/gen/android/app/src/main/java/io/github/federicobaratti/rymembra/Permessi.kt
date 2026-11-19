package io.github.federicobaratti.rymembra

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Looper
import android.provider.Settings
import android.util.Log
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

/**
 * Che cosa l'app può leggere.
 *
 * Due livelli, perché Android ne ha due. I permessi «media» (audio, immagini)
 * si chiedono con un dialogo e bastano a leggere i brani e le copertine.
 * «Accesso a tutti i file» si concede da una pagina delle impostazioni, ed è
 * quel che serve per il resto di una libreria vera: i `.lrc` e i `.cue` accanto
 * ai brani (e i `.lrc` dei testi sincronizzati, se lo si chiede), le copertine
 * accanto, i file da eliminare. I tag dei file che ci sono Rymembra non li
 * riscrive: scrive soltanto quelli dei brani che scarica.
 */
object Permessi {
  private fun concesso(contesto: Context, permesso: String) =
    ContextCompat.checkSelfPermission(contesto, permesso) == PackageManager.PERMISSION_GRANTED

  fun stato(contesto: Context): JSONObject {
    val tutti =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
      else concesso(contesto, Manifest.permission.WRITE_EXTERNAL_STORAGE)
    val audio =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) concesso(contesto, Manifest.permission.READ_MEDIA_AUDIO)
      else concesso(contesto, Manifest.permission.READ_EXTERNAL_STORAGE)
    // Da Android 13 le immagini fuori dai tag le legge solo «Accesso a tutti i
    // file»: il permesso delle foto Rymembra non lo chiede.
    val immagini =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) false
      else concesso(contesto, Manifest.permission.READ_EXTERNAL_STORAGE)
    return JSONObject()
      .put("tuttiIFile", tutti)
      .put("audio", audio || tutti)
      .put("immagini", immagini || tutti)
      .put("notifiche", NotificationManagerCompat.from(contesto).areNotificationsEnabled())
  }

  /**
   * I permessi del dialogo per leggere la musica, e lo stato dopo la risposta.
   *
   * Le notifiche no: la schermata che porta qui spiega l'accesso ai file, e un
   * secondo dialogo di cui non parla — «Inviare notifiche?» — confonde chi sta
   * rispondendo al primo. La notifica del lettore non ne ha bisogno (Android
   * esenta quelle delle sessioni multimediali); le altre si chiedono quando
   * servono davvero, in [notificheAlBisogno].
   */
  fun chiedi(attivita: MainActivity): JSONObject {
    val chiesti = buildList {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        // Le immagini no: per un lettore la domanda «foto e video» insospettisce
        // a ragione, e le copertine accanto ai brani le legge già «Accesso a
        // tutti i file». Quelle dentro i tag arrivano con il brano.
        add(Manifest.permission.READ_MEDIA_AUDIO)
      } else {
        add(Manifest.permission.READ_EXTERNAL_STORAGE)
        if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.Q) add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
      }
    }
    attivita.aspettaPermessi(chiesti.toTypedArray())
    return stato(attivita)
  }

  /** Le notifiche sono già state chieste, in questa vita del processo. */
  @Volatile private var notificheChieste = false

  /**
   * Le notifiche, al primo bisogno vero (D-36): quando parte «Scarica qui» o
   * l'attesa del consenso di un servizio da collegare. Sono i due servizi in
   * primo piano che hanno una notifica da mostrare oltre a quella del lettore:
   * senza il permesso lavorano lo stesso, ma la loro notifica finisce solo nel
   * Task Manager. Si chiedono qui, nel momento in cui chi tocca il tasto sa
   * perché arriva la domanda, e non al primo avvio.
   *
   * Una volta per processo: chi ha detto di no non se lo sente richiedere a
   * ogni brano scaricato, e Android stesso smette di mostrare il dialogo dopo
   * due rifiuti. Da Android 13: prima il permesso non c'è.
   *
   * Con `aspetta` si aspetta la risposta — il consenso, che apre il browser
   * subito dopo e coprirebbe il dialogo —, ma mai dal filo principale, che
   * aspetterebbe se stesso; senza, la domanda parte e chi chiama va avanti:
   * lo scaricamento non si ferma ad aspettare un dialogo.
   */
  fun notificheAlBisogno(attivita: MainActivity?, aspetta: Boolean) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
    if (attivita == null || attivita.isFinishing || attivita.isDestroyed) return
    if (concesso(attivita, Manifest.permission.POST_NOTIFICATIONS)) return
    synchronized(this) {
      if (notificheChieste) return
      notificheChieste = true
    }
    val chiesti = arrayOf(Manifest.permission.POST_NOTIFICATIONS)
    if (aspetta && Looper.myLooper() != Looper.getMainLooper()) {
      try {
        attivita.aspettaPermessi(chiesti)
      } catch (e: Exception) {
        // Un dialogo che non si apre non ferma quel che lo ha chiesto.
        Log.w("Rymembra", "le notifiche non si chiedono: ${e.javaClass.simpleName}")
      }
    } else {
      attivita.runOnUiThread { ActivityCompat.requestPermissions(attivita, chiesti, CODICE_NOTIFICHE) }
    }
  }

  /**
   * Il codice della richiesta delle notifiche che non aspetta la risposta: la
   * risposta arriva a [MainActivity.onRequestPermissionsResult], che rifà la
   * notifica di [Presenza] se il sottofondo era già acceso.
   */
  const val CODICE_NOTIFICHE = 4_213

  /**
   * La rete di casa. Da Android 17 un'app che si dichiara per Android 17 non
   * la vede finché non le si concede «Dispositivi nelle vicinanze»: i pacchetti
   * verso gli indirizzi della Wi-Fi — e quelli che arrivano da lì — si fermano
   * nel sistema, senza errori che dicano perché. È quel che serve ad «Ascolta
   * insieme». Prima di Android 17 il permesso non esiste e la rete è aperta.
   */
  fun reteLocale(contesto: Context): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.CINNAMON_BUN || concesso(contesto, Manifest.permission.ACCESS_LOCAL_NETWORK)

  /** Chiede la rete di casa col dialogo di Android, e dice com'è andata. */
  fun chiediReteLocale(attivita: MainActivity): Boolean {
    if (reteLocale(attivita)) return true
    attivita.aspettaPermessi(arrayOf(Manifest.permission.ACCESS_LOCAL_NETWORK))
    return reteLocale(attivita)
  }

  /**
   * La pagina di «Accesso a tutti i file». Fino ad Android 10 non esiste: lì
   * bastano i permessi del dialogo, con `requestLegacyExternalStorage`.
   */
  fun chiediTuttiIFile(attivita: MainActivity) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    attivita.runOnUiThread {
      try {
        attivita.startActivity(
          Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${attivita.packageName}"))
        )
      } catch (_: ActivityNotFoundException) {
        // Qualche produttore non ha la pagina della singola app: quella generale
        // c'è sempre.
        attivita.startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
      }
    }
  }
}
