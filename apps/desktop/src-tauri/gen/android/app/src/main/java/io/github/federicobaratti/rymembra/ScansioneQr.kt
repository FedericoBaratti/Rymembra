package io.github.federicobaratti.rymembra

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import org.json.JSONObject
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Lo scanner del QR di «Collega un dispositivo».
 *
 * CameraX per l'anteprima e i fotogrammi, ZXing (solo il nucleo, Apache-2.0)
 * per leggerli: niente Play Services, che un telefono senza Google non ha. Si
 * legge solo il piano della luminanza, che a un QR basta, e solo i QR.
 *
 * Restituisce il primo link `rymembra://collega/…` che vede; il resto lo
 * controlla Rust (`Invito::da_link`). La fotocamera si chiede qui, al primo
 * uso: chi non la concede ha ancora «Incolla link».
 *
 * La lancia [scansiona], dal ponte (`sistema.scansionaQr`), con
 * `MainActivity.aspettaRisultato`, come i selettori del sistema.
 */
class ScansioneQr : AppCompatActivity() {
  private lateinit var analisi: ExecutorService
  private lateinit var vista: PreviewView
  private lateinit var torcia: Button
  private var camera: Camera? = null
  private var torciaAccesa = false
  private val trovato = AtomicBoolean(false)
  private val lettore = QRCodeReader()
  private val suggerimenti = mapOf(
    DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
    DecodeHintType.TRY_HARDER to true,
  )

  private val permesso =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) { concesso ->
      if (concesso) {
        avvia()
      } else {
        setResult(NEGATO)
        finish()
      }
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    analisi = Executors.newSingleThreadExecutor()
    setContentView(schermata())
    if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
      avvia()
    } else {
      permesso.launch(Manifest.permission.CAMERA)
    }
  }

  override fun onDestroy() {
    analisi.shutdown()
    super.onDestroy()
  }

  /** L'anteprima a tutto schermo, l'istruzione in alto, i tasti in basso. */
  private fun schermata(): FrameLayout {
    val radice = FrameLayout(this).apply { setBackgroundColor(Color.BLACK) }
    vista = PreviewView(this)
    radice.addView(vista, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))

    val margine = (24 * resources.displayMetrics.density).toInt()
    val istruzioni = TextView(this).apply {
      setText(R.string.qr_istruzioni)
      setTextColor(Color.WHITE)
      textSize = 17f
      gravity = Gravity.CENTER
      setShadowLayer(6f, 0f, 0f, Color.BLACK)
      setPadding(margine, margine, margine, margine)
    }
    radice.addView(
      istruzioni,
      FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP),
    )

    torcia = Button(this).apply {
      setText(R.string.qr_torcia)
      isEnabled = false
      setOnClickListener {
        torciaAccesa = !torciaAccesa
        camera?.cameraControl?.enableTorch(torciaAccesa)
      }
    }
    val annulla = Button(this).apply {
      setText(R.string.qr_annulla)
      setOnClickListener {
        setResult(Activity.RESULT_CANCELED)
        finish()
      }
    }
    val tasti = LinearLayout(this).apply {
      orientation = LinearLayout.HORIZONTAL
      gravity = Gravity.CENTER
      setPadding(margine, margine, margine, margine)
      addView(torcia)
      addView(annulla)
    }
    radice.addView(
      tasti,
      FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM),
    )

    // Le barre del sistema: le istruzioni sotto quella di stato, i tasti
    // sopra quella dei gesti.
    ViewCompat.setOnApplyWindowInsetsListener(radice) { _, riquadri ->
      val barre = riquadri.getInsets(WindowInsetsCompat.Type.systemBars())
      istruzioni.setPadding(margine, margine + barre.top, margine, margine)
      tasti.setPadding(margine, margine, margine, margine + barre.bottom)
      riquadri
    }
    return radice
  }

  private fun avvia() {
    val futuro = ProcessCameraProvider.getInstance(this)
    futuro.addListener({
      if (isFinishing || isDestroyed) return@addListener
      try {
        val fornitore = futuro.get()
        val anteprima = Preview.Builder().build()
        anteprima.setSurfaceProvider(vista.surfaceProvider)
        val fotogrammi = ImageAnalysis.Builder()
          .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
          .build()
        fotogrammi.setAnalyzer(analisi) { immagine -> leggi(immagine) }
        fornitore.unbindAll()
        camera = fornitore.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, anteprima, fotogrammi)
        torcia.isEnabled = camera?.cameraInfo?.hasFlashUnit() == true
      } catch (e: Exception) {
        // Nessuna fotocamera posteriore, o una che non si apre: si torna
        // indietro, e resta il link da incollare.
        setResult(GUASTO)
        finish()
      }
    }, ContextCompat.getMainExecutor(this))
  }

  /** Un fotogramma: la luminanza, e un QR se c'è. Sul filo dell'analisi. */
  private fun leggi(immagine: ImageProxy) {
    immagine.use {
      if (trovato.get()) return
      val piano = it.planes[0]
      val buffer = piano.buffer
      val passo = piano.rowStride
      // L'ultima riga può essere più corta del passo: il buffer non arriva
      // sempre a `passo * altezza`, e il resto resta a zero.
      val dati = ByteArray(passo * it.height)
      buffer.rewind()
      buffer.get(dati, 0, minOf(buffer.remaining(), dati.size))
      val luminanza = PlanarYUVLuminanceSource(dati, passo, it.height, 0, 0, it.width, it.height, false)
      val testo = try {
        lettore.decode(BinaryBitmap(HybridBinarizer(luminanza)), suggerimenti).text
      } catch (_: ReaderException) {
        null
      } finally {
        lettore.reset()
      }
      if (testo != null && testo.startsWith(PREFISSO) && trovato.compareAndSet(false, true)) {
        runOnUiThread {
          setResult(Activity.RESULT_OK, Intent().putExtra(LINK, testo))
          finish()
        }
      }
    }
  }

  companion object {
    private const val PREFISSO = "rymembra://collega/"
    private const val LINK = "link"
    private const val NEGATO = Activity.RESULT_FIRST_USER
    private const val GUASTO = Activity.RESULT_FIRST_USER + 1

    /**
     * Apre lo scanner e ne aspetta l'esito: `{"link": "…" | null, "negato":
     * bool}`. Da un filo che non sia il principale (vedi
     * `MainActivity.aspettaRisultato`).
     */
    fun scansiona(attivita: MainActivity): JSONObject {
      val risultato = attivita.aspettaRisultato(Intent(attivita, ScansioneQr::class.java))
      val link = if (risultato.resultCode == Activity.RESULT_OK) risultato.data?.getStringExtra(LINK) else null
      return JSONObject()
        .put("link", link ?: JSONObject.NULL)
        .put("negato", risultato.resultCode == NEGATO)
    }
  }
}
