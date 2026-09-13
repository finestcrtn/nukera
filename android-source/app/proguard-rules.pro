# Keep JNI methods
-keepclasseswithmembernames class * {
    native <methods>;
}

-keep class app.nukera.core.NukeraProxy { *; }

-keep,allowoptimization class app.nukera.core.TProxyService { *; }
-keep,allowoptimization class app.nukera.activities.** { *; }
-keep,allowoptimization class app.nukera.services.** { *; }
-keep,allowoptimization class app.nukera.receiver.** { *; }

-keep class app.nukera.fragments.** {
    <init>();
}

-keep,allowoptimization class app.nukera.data.** {
    <fields>;
}

-keepattributes Signature
-keepattributes *Annotation*

-repackageclasses 'ru.romanvht'
-renamesourcefileattribute ''
-keepattributes SourceFile,InnerClasses,EnclosingMethod,Signature,RuntimeVisibleAnnotations,*Annotation*,*Parcelable*
-allowaccessmodification
-overloadaggressively
-optimizationpasses 5
-verbose
-dontusemixedcaseclassnames
-adaptclassstrings
-adaptresourcefilecontents **.xml,**.json
-adaptresourcefilenames **.xml,**.json