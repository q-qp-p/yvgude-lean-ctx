import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm")
    id("org.jetbrains.intellij.platform")
}

// group + version are defined once in gradle.properties (single source of truth).
// The Release workflow overrides the version from the git tag via `-Pversion=<x.y.z>`
// so the published plugin always mirrors the lean-ctx release it ships with.

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    compileOnly("com.google.code.gson:gson:2.11.0")
    testImplementation("com.google.code.gson:gson:2.11.0")
    testImplementation("junit:junit:4.13.2")
    intellijPlatform {
        intellijIdea("2026.1.3")
        bundledPlugin("org.jetbrains.kotlin")
        testFramework(TestFrameworkType.Platform)
    }
}

intellijPlatform {
    pluginConfiguration {
        name = "lean-ctx"
        version = project.version.toString()
        ideaVersion {
            sinceBuild = "261"
            // untilBuild intentionally left open (private plugin, no Marketplace).
        }
        vendor {
            name = "lean-ctx"
            url = "https://github.com/yvgude/lean-ctx"
        }
    }
}

kotlin {
    jvmToolchain(21)
    compilerOptions {
        jvmTarget = JvmTarget.JVM_21
    }
}

intellijPlatformTesting {
    runIde {
        register("runRustRover") {
            type = IntelliJPlatformType.RustRover
            version = "2026.1"
        }
        register("runPyCharm") {
            type = IntelliJPlatformType.PyCharmCommunity
            version = "2026.1"
        }
    }
}
