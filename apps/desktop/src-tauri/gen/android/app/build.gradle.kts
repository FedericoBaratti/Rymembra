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

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
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
