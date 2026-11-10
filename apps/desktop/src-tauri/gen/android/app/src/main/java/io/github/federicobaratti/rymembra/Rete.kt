package io.github.federicobaratti.rymembra

import android.content.Context
import android.net.ConnectivityManager
import android.os.Build
import android.net.Network
import android.net.wifi.WifiManager

/**
 * La rete, per «Ascolta insieme» e per il collegamento via internet.
 *
 * Per risparmiare batteria molti telefoni scartano i pacchetti mandati a tutta
 * la Wi-Fi, e sono proprio quelli con cui un Rymembra trova gli altri. Il
 * lucchetto del multicast li fa passare, e si tiene solo finché la funzione è
 * accesa.
 *
 * Il Wi-Fi, poi, a schermo spento si addormenta fra un pacchetto e l'altro:
 * per una pagina web va bene, per l'audio che arriva da un altro dispositivo
 * no — l'orologio di una sessione «Ascolta insieme» salta, e un brano del PC
 * sui dati di casa si ferma ad aspettare. `wifi` tiene il Wi-Fi sveglio
 * finché qualcuno (`insieme`, `lontano`) lo chiede.
 */
object Rete {
  private var lucchetto: WifiManager.MulticastLock? = null
  private var osservata = false
  private val sveglioPer = mutableSetOf<String>()
  private var bassaLatenza: WifiManager.WifiLock? = null
  private var altePrestazioni: WifiManager.WifiLock? = null

  /**
   * Segue la rete di sistema, una volta per processo: quando cambia — il Wi-Fi
   * di casa che se ne va, il 5G che arriva — lo si dice a Rust, e il
   * collegamento via internet cerca strade nuove invece di aspettare che le
   * vecchie scadano. La prima rete, che arriva appena ci si registra, non è un
   * cambio.
   */
  @Synchronized
  fun osserva(contesto: Context) {
    if (osservata) return
    osservata = true
    val connessioni = contesto.applicationContext
      .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    connessioni.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
      private var attuale: Network? = null
      private var prima = true

      override fun onAvailable(network: Network) {
        if (network == attuale) return
        attuale = network
        if (prima) {
          prima = false
          return
        }
        Sessione.cambioDiRete()
      }

      override fun onLost(network: Network) {
        if (network != attuale) return
        attuale = null
        Sessione.cambioDiRete()
      }
    })
  }

  /**
   * La rete di adesso è a consumo — i dati mobili, un hotspot — per «Solo
   * Wi-Fi». Senza una rete si risponde di no: non c'è niente da consumare, e
   * il collegamento se ne accorgerà da sé.
   */
  fun aConsumo(contesto: Context): Boolean {
    val connessioni = contesto.applicationContext
      .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    // Senza rete `isActiveNetworkMetered` dice di sì: lo si guarda solo con
    // una rete attiva.
    if (connessioni.activeNetwork == null) return false
    return connessioni.isActiveNetworkMetered
  }

  /**
   * Tiene il Wi-Fi sveglio per `chi`, o lo lascia. Il lucchetto è uno, e si
   * lascia quando nessuno lo chiede più.
   *
   * Due lucchetti, perché Android ne ha cambiato il senso due volte: quello a
   * bassa latenza (API 29+) vale solo con l'app in vista, e quello ad alte
   * prestazioni — l'unico che vale a schermo spento — dall'API 34 non fa più
   * niente. Si prende quel che il sistema onora.
   */
  @Synchronized
  @Suppress("DEPRECATION")
  fun wifi(contesto: Context, chi: String, acceso: Boolean) {
    if (acceso) sveglioPer.add(chi) else sveglioPer.remove(chi)
    if (sveglioPer.isEmpty()) {
      bassaLatenza?.takeIf { it.isHeld }?.release()
      altePrestazioni?.takeIf { it.isHeld }?.release()
      bassaLatenza = null
      altePrestazioni = null
      return
    }
    val wifi = contesto.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && bassaLatenza?.isHeld != true) {
      bassaLatenza = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "rymembra:rete").apply {
        setReferenceCounted(false)
        acquire()
      }
    }
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE && altePrestazioni?.isHeld != true) {
      altePrestazioni = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "rymembra:rete-sfondo").apply {
        setReferenceCounted(false)
        acquire()
      }
    }
  }

  @Synchronized
  fun multicast(contesto: Context, acceso: Boolean) {
    if (acceso) {
      if (lucchetto?.isHeld == true) return
      val wifi = contesto.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
      lucchetto = wifi.createMulticastLock("rymembra-insieme").apply {
        setReferenceCounted(false)
        acquire()
      }
    } else {
      lucchetto?.takeIf { it.isHeld }?.release()
      lucchetto = null
    }
  }
}
