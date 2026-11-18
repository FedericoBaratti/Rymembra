import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// La chiave con cui si firma l'APK di rilascio. Sta **fuori** dal repository
// (`keystore.properties` e il file `.jks` sono nei `.gitignore`), ed è come la
// chiave dell'updater sul computer: Android installa un APK sopra un altro solo
// se è firmato con la stessa chiave, quindi perderla vuol dire che chi ha già
// Rymembra deve disinstallarla — con la sua libreria — per avere la versione
// dopo. Una copia fuori da questo computer.
//
// keystore.properties, accanto a questa cartella (`gen/android/`):
//   storeFile=C:/percorso/rymembra-rilascio.jks
//   password=…
//   keyAlias=rymembra
//
// Senza, il rilascio esce non firmato e Android non lo installa; il debug si
// firma da sé con la chiave di prova di Gradle, come sempre.
val firma = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// ── due distribuzioni, una proprietà ────────────────────────────────────────
//
// `github` (di serie): l'APK delle release di GitHub, con l'aggiornatore di
// sempre — scarica l'APK nuovo e lo consegna all'installatore di Android — e
// il permesso `REQUEST_INSTALL_PACKAGES` che gli serve.
// `play`: la build per il Google Play Store. Per la policy gli aggiornamenti
// passano solo dal Play, e `REQUEST_INSTALL_PACKAGES` è un permesso ristretto:
// qui non c'è né l'aggiornatore né il permesso, e il nucleo (Rust) lo sa dalla
// costante `BuildConfig.DISTRIBUZIONE`.
//
//   gradlew … -Pdistribuzione=play
//   ORG_GRADLE_PROJECT_distribuzione=play  npx tauri android build …
//
// La seconda forma è quella che serve alla CLI di Tauri, che non inoltra
// proprietà a Gradle: Gradle legge dall'ambiente le variabili
// `ORG_GRADLE_PROJECT_<nome>`.
//
// **Perché una proprietà e non una flavor.** La CLI di Tauri dà per scontata la
// forma di questo progetto: una sola dimensione di flavor, `abi` (la crea il
// plugin `rust` di `buildSrc`), compiti `assemble<Arco><Profilo>` e un APK in
// `outputs/apk/<arco>/<profilo>/`. Una seconda dimensione — `distribuzione` con
// `github` e `play` — ha fatto fallire Gradle già alla configurazione:
// «Task with name 'mergeUniversalDebugJniLibFolders' not found in project ':app'»,
// perché il plugin cerca i compiti con il nome a una dimensione sola. E anche se
// non fallisse, cambierebbero i nomi dei compiti (`assembleArm64GithubRelease`) e
// le cartelle d'uscita, cioè la CI (`release.yml`, `strumenti/apk.js`) e il
// lint (`lintArm64Debug`). Con la proprietà i nomi restano quelli di oggi.
val distribuzione: String = (findProperty("distribuzione") as? String) ?: "github"
require(distribuzione == "github" || distribuzione == "play") {
    "distribuzione: «$distribuzione» non è né github né play"
}
/**
 * Questa è la build per il Play Store. Una costante sola, per chi deve decidere
 * di una cosa in base alla distribuzione: l'aggiornatore, il permesso e le
 * costanti del blocco più sotto (`DISTRIBUZIONE`, `AGGIORNATORE_APK`, e la
 * partenza con l'auto, `PARTENZA_CON_AUTO`, che il `defaultConfig` dichiara
 * `true` e qui si riscrive a `false`).
 */
val distribuzionePlay: Boolean = distribuzione == "play"

android {
    compileSdk = 37
    namespace = "io.github.federicobaratti.rymembra"
    signingConfigs {
        if (firma.containsKey("storeFile")) {
            create("rilascio") {
                storeFile = file(firma.getProperty("storeFile"))
                storePassword = firma.getProperty("password")
                keyAlias = firma.getProperty("keyAlias")
                keyPassword = firma.getProperty("password")
            }
        }
    }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "io.github.federicobaratti.rymembra"
        // Android 8: AAudio, l'uscita audio di `rymembra-play` (vedi
        // `tauri.android.conf.json` e `rust-toolchain.toml`).
        minSdk = 26
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        // «Parti con l'auto»: quando l'auto si collega, Rymembra riprende da
        // sola la coda salvata, al posto dell'ultima app usata. È spenta di
        // serie e solo l'utente la accende, ma le linee guida di qualità di
        // Google per le app multimediali delle auto — `MA-1`, «The app must not
        // autoplay on startup or without user initiated action to select the
        // app or app media» (developer.android.com/docs/quality-guidelines/car-app-quality)
        // — e la pagina «Enable playback controls» («Your app mustn't
        // automatically start playing music when Android Automotive OS or
        // Android Auto connect to the media browser service») dicono il
        // contrario, e la revisione di Play può contestarla. Questa è l'**unica**
        // costante che la toglie: Kotlin la legge (`Presenza`, `Ponte`) e la passa
        // alla finestra, che dove è falsa non disegna l'interruttore. La variante
        // per Play la mette a `false`; quella di GitHub resta com'è.
        buildConfigField("boolean", "PARTENZA_CON_AUTO", "true")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfigs.findByName("rilascio")?.let { signingConfig = it }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

// Le due costanti della distribuzione e, sul Play, il manifesto senza il permesso.
// Un blocco a parte: tiene separato quel che dipende dalla proprietà dal resto
// della configurazione, che è quella di Tauri.
android {
    defaultConfig {
        // `github` o `play`: il nucleo la legge con `sistema.distribuzione` e
        // spegne l'aggiornatore del telefono (Rust e finestra) sul Play.
        buildConfigField("String", "DISTRIBUZIONE", "\"$distribuzione\"")
        // L'aggiornatore APK c'è solo fuori dal Play: `Installatore` lo controlla.
        buildConfigField("boolean", "AGGIORNATORE_APK", (!distribuzionePlay).toString())
    }
    if (distribuzionePlay) {
        // «Parti con l'auto» (F7) è spenta sul Play: la regola di qualità di Google
        // per le app delle auto, `MA-1`, vieta di partire da sole quando l'auto si
        // collega, e la revisione di Play può contestare anche un'opzione dell'utente.
        // Questa riscrive la costante che il `defaultConfig` dichiara `true`: è
        // l'unica che la toglie, e Kotlin la passa alla finestra, che senza di lei non
        // disegna l'interruttore.
        defaultConfig {
            buildConfigField("boolean", "PARTENZA_CON_AUTO", "false")
        }
        // Un manifesto che si somma a quello di `main` e toglie
        // `REQUEST_INSTALL_PACKAGES`. Sta in una cartella che Android Gradle
        // **non** conosce (`src/debug` e `src/release` le prenderebbe da sola,
        // anche senza la proprietà) e si attacca ai due tipi di build solo qui.
        // Un manifesto di tipo di build vince su quello di `main`: è l'ordine di
        // priorità del fusore.
        sourceSets.getByName("debug").manifest.srcFile("src/distribuzione-play/AndroidManifest.xml")
        sourceSets.getByName("release").manifest.srcFile("src/distribuzione-play/AndroidManifest.xml")
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

// Gli avvisi di licenza dentro l'APK, in `assets/legale/`: gli stessi file che
// l'installer del computer mette accanto all'eseguibile (`bundle.resources` di
// `tauri.conf.json`), con gli stessi nomi. La MIT chiede l'avviso «in all
// copies», l'Apache-2.0 una copia della licenza a chi riceve il programma, la
// OFL la licenza accanto ai font: un link a GitHub, che senza rete non si apre
// e porta al file di `main`, non bastava. I percorsi partono da questa cartella
// (`gen/android`): la radice del repository è cinque cartelle sopra.
val avvisiDellApk = mapOf(
    "LICENSE.txt" to "../../../../../LICENSE",
    "THIRD-PARTY-NOTICES.md" to "../../../../../THIRD-PARTY-NOTICES.md",
    "PRIVACY.md" to "../../../../../PRIVACY.md",
    "TERMS.md" to "../../../../../TERMS.md",
    "font-OFL.txt" to "../../../src/font/OFL.txt",
)

/** Copia gli avvisi in una cartella che diventa una sorgente di asset. */
abstract class AvvisiLegali : DefaultTask() {
    /** Il nome dentro l'APK, e il file da cui viene. */
    @get:Input
    abstract val avvisi: MapProperty<String, String>

    /** Gli stessi file, perché un avviso cambiato rifaccia la copia. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val sorgenti: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val cartella: DirectoryProperty

    @TaskAction
    fun copia() {
        val legale = cartella.get().asFile.resolve("legale")
        legale.deleteRecursively()
        legale.mkdirs()
        avvisi.get().forEach { (nome, sorgente) ->
            // Un avviso che manca ferma la build: un APK senza è proprio quel
            // che questo compito esiste per non pubblicare.
            File(sorgente).copyTo(legale.resolve(nome))
        }
    }
}

val avvisiLegali = tasks.register<AvvisiLegali>("avvisiLegali") {
    avvisi.set(avvisiDellApk.mapValues { (_, sorgente) -> rootProject.file(sorgente).absolutePath })
    sorgenti.from(avvisiDellApk.values.map { rootProject.file(it) })
}

androidComponents {
    onVariants { variante ->
        variante.sources.assets?.addGeneratedSourceDirectory(avvisiLegali, AvvisiLegali::cartella)
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // La sessione multimediale: la notifica del lettore, la schermata di
    // blocco, i tasti delle cuffie, e il servizio in primo piano che tiene viva
    // la musica a schermo spento. Vedi `Sessione.kt`.
    implementation("androidx.media3:media3-session:1.8.0")
    implementation("androidx.media3:media3-common:1.8.0")
    // «Parti con l'auto»: `CarConnection`, che dice se il telefono è in
    // un'auto con Android Auto (`CONNECTION_TYPE_PROJECTION`). Solo quella
    // classe, via `Presenza.kt`: Rymembra non è un'app «per auto» della Car App
    // Library, e la sua sessione resta quella di Media3. Apache-2.0 (vedi
    // `THIRD-PARTY-NOTICES.md`). La 1.7.0 è l'ultima stabile (la 1.8.0 è in
    // release candidate); il suo `minSdk` è 21, il nostro 26.
    implementation("androidx.car.app:app:1.7.0")
    // «Collega un dispositivo»: la fotocamera per il QR mostrato dal PC
    // (CameraX), e la sua lettura (ZXing core, Apache-2.0: niente Play
    // Services, che un telefono senza Google non ha). Vedi `ScansioneQr.kt`.
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    implementation("com.google.zxing:core:3.5.3")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
