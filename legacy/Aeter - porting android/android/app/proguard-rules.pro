# R8 keep rules for the release build (minifyEnabled true in build.gradle).
# Everything reached via reflection or JNI must keep its runtime name here;
# the rest of the app/AndroidX graph is shrunk and optimized by R8.

# --- Capacitor bridge ---------------------------------------------------
# Plugin discovery + method dispatch happen via reflection on the
# @CapacitorPlugin annotation and @PluginMethod-annotated methods.
-keep class com.getcapacitor.** { *; }
-keep @com.getcapacitor.annotation.CapacitorPlugin class * {
    @com.getcapacitor.annotation.PermissionCallback <methods>;
    @com.getcapacitor.annotation.ActivityCallback <methods>;
    @com.getcapacitor.PluginMethod public <methods>;
}

# --- Aether native plugins + services ------------------------------------
# Registered by class name (MainActivity.registerPlugin) and invoked through
# the Capacitor reflection dispatch above; services/receivers are also
# referenced from the manifest.
-keep class com.aether.player.** { *; }

# --- Cordova + nodejs-mobile ---------------------------------------------
# The Cordova bridge instantiates plugins from config.xml by name, and
# libnodejs-mobile-cordova-native-lib.so resolves Java callbacks through JNI
# — both need stable class/method names.
-keep class org.apache.cordova.** { *; }
-keep class com.janeasystems.cdvnodejsmobile.** { *; }
-keepclasseswithmembers,includedescriptorclasses class * {
    native <methods>;
}

# --- youtubedl-android (bundled Python/ffmpeg wrapper) --------------------
-keep class com.yausername.** { *; }
-dontwarn com.yausername.**

# --- Media notification --------------------------------------------------
# Manifest-declared receiver resolved by name from PendingIntents.
-keep class androidx.media.session.MediaButtonReceiver { *; }

# Readable stack traces from release crashes (mapping.txt still applies).
-keepattributes SourceFile,LineNumberTable
