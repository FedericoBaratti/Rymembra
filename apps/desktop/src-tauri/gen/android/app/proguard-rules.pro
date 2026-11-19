# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
# Le tre funzioni di Rust che il servizio chiama per avviare il nucleo senza
# attività (`Ponte.avviaNucleo`, `rilasciaIlContestoDelServizio`,
# `contestoDiTaoPronto`): JNI le cerca per nome (`Java_…_Ponte_…`), e R8 le
# rinominerebbe — il file non include le regole di serie di Android, che tengono i
# metodi `native`. Rinominate, la prima chiamata darebbe `UnsatisfiedLinkError`,
# solo nella build di rilascio.
-keepclasseswithmembernames class io.github.federicobaratti.rymembra.Ponte {
    native <methods>;
}

# Il ponte con Rust: lo trova JNI per nome (`rymembra-android/src/jvm.rs`), da
# un caricatore di classi che R8 non vede. Rinominato o tolto, la prima
# chiamata da Rust fallirebbe con `ClassNotFoundException`.
-keep class io.github.federicobaratti.rymembra.Ponte {
    public static java.lang.String chiama(java.lang.String, java.lang.String);
}

# Due metodi della WebView che wry chiama per nome (`main_pipe.rs`, per
# `clear_all_browsing_data` e `cookies_for_url`) e che `proguard-wry.pro` non
# tiene: R8 li toglie (`usage.txt` della release di prova). Oggi Rymembra non li
# usa, quindi non si vede; il giorno che li usasse, la chiamata cadrebbe con un
# `NoSuchMethodError` solo nella build di rilascio.
-keep class io.github.federicobaratti.rymembra.RustWebView {
    void clearAllBrowsingData();
    java.lang.String getCookies(java.lang.String);
}

# Le annotazioni che Tauri legge per riflessione (`@TauriPlugin`, `@Command`,
# in `PluginHandle.kt`): senza, i comandi dei plugin rispondono «No command …
# found». Oggi restano perché qualche libreria lo chiede per sé; scritto qui non
# dipende più da loro.
-keepattributes RuntimeVisibleAnnotations,AnnotationDefault
