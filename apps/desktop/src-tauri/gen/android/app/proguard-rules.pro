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
# Il ponte con Rust: lo trova JNI per nome (`rymembra-android/src/jvm.rs`), da
# un caricatore di classi che R8 non vede. Rinominato o tolto, la prima
# chiamata da Rust fallirebbe con `ClassNotFoundException`.
-keep class io.github.federicobaratti.rymembra.Ponte {
    public static java.lang.String chiama(java.lang.String, java.lang.String);
}
