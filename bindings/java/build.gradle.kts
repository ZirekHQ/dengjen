import org.gradle.internal.os.OperatingSystem






buildscript {
    configurations.classpath {
        resolutionStrategy {
            force("org.eclipse.jgit:org.eclipse.jgit:5.13.5.202508271544-r")
        }
    }
}

plugins {
    java
    jacoco
    `jvm-test-suite`
    `maven-publish`
    alias(libs.plugins.git.version)
    alias(libs.plugins.jreleaser)
    alias(libs.plugins.spotless)
}

group = "io.github.zirekhq"
version = "0.0.0-SNAPSHOT"

val snapshotVersion =
    "\${describe.tag.version.major}.\${describe.tag.version.minor}.\${describe.tag.version.patch.next}-SNAPSHOT"

gitVersioning.apply {
    refs {
        branch("main") { version = snapshotVersion }
        tag("java-v(?<version>.*)") { version = "\${ref.version}" }
    }
    
    
    describeTagPattern = "java-v.*"
    rev { version = snapshotVersion }
}

fun currentNativeClassifier(): String {
    val os = OperatingSystem.current()
    val arch = System.getProperty("os.arch").lowercase()
    val isArm64 = arch == "aarch64" || arch == "arm64"
    val isX64 = arch == "x86_64" || arch == "amd64" || arch == "x64"
    return when {
        os.isWindows && isX64 -> "windows-x64"
        os.isMacOsX && isArm64 -> "macos-aarch64"
        os.isLinux && isArm64 -> "linux-aarch64"
        os.isLinux && isX64 -> "linux-x86_64"
        else -> throw GradleException("nativeTestJar: unsupported platform (os=$os, arch=$arch)")
    }
}





val nativeTestJar by tasks.registering(Jar::class) {
    archiveBaseName.set("dengjen-java-bindings-native-test")
    archiveClassifier.set(currentNativeClassifier())
    val nativeLibraryFile = file("../../target/release/${System.mapLibraryName("libdengjen")}")
    from(nativeLibraryFile) { into("natives/${currentNativeClassifier()}") }
    doFirst {
        check(nativeLibraryFile.exists()) {
            "libdengjen not built at $nativeLibraryFile -- run `make native` first"
        }
    }
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
    withSourcesJar()
    withJavadocJar()
}

// The javadoc tool copies ~4 MB of DejaVu fonts into every output; browsers fall back to system fonts.
tasks.named<Jar>("javadocJar") {
    exclude("resource-files/fonts/**")
}

repositories {
    mavenCentral()
}

dependencyLocking {
    lockAllConfigurations()
}

testing {
    suites {
        val test by getting(JvmTestSuite::class) {
            dependencies {
                implementation(libs.assertj.core)
            }
            useJUnitJupiter(libs.versions.junit.jupiter.get())
        }
        val integrationTest by registering(JvmTestSuite::class) {
            dependencies {
                implementation(project())
                implementation(libs.assertj.core)
                implementation(libs.awaitility)
            }
            useJUnitJupiter(libs.versions.junit.jupiter.get())
            targets {
                all {
                    testTask.configure {
                        shouldRunAfter(test)
                    }
                }
            }
        }
        
        
        
        val e2e by registering(JvmTestSuite::class) {
            dependencies {
                implementation(project())
                implementation(libs.assertj.core)
            }
            useJUnitJupiter(libs.versions.junit.jupiter.get())
            targets {
                all {
                    testTask.configure {
                        shouldRunAfter(integrationTest)
                    }
                }
            }
        }
        val nativeLoaderTest by registering(JvmTestSuite::class) {
            dependencies {
                implementation(project())
                implementation(libs.assertj.core)
            }
            useJUnitJupiter(libs.versions.junit.jupiter.get())
            targets {
                all {
                    testTask.configure {
                        dependsOn(nativeTestJar)
                        
                        
                        
                        classpath += files(nativeTestJar.map { it.archiveFile })
                        jvmArgs("--enable-native-access=ALL-UNNAMED")
                        shouldRunAfter(tasks.test)
                    }
                }
            }
        }
    }
}

tasks.withType<Test>().configureEach {
    jvmArgs("--enable-native-access=ALL-UNNAMED")
}





val devNativeLibraryPath = file("../../target/release/${System.mapLibraryName("libdengjen")}").absolutePath

listOf("test", "integrationTest", "e2e").forEach { suiteName ->
    tasks.named<Test>(suiteName) {
        systemProperty("dengjen.native.library.path", devNativeLibraryPath)
    }
}





tasks.jacocoTestReport {
    dependsOn(tasks.test, testing.suites.named("integrationTest"), testing.suites.named("e2e"))
    executionData(fileTree(layout.buildDirectory.dir("jacoco")).include("*.exec"))
    reports {
        xml.required.set(true)
        html.required.set(false)
    }
}

spotless {
    java {
        googleJavaFormat()
    }
}

val stagingDir: Provider<Directory> = layout.buildDirectory.dir("staging-deploy")

publishing {
    publications {
        create<MavenPublication>("maven") {
            from(components["java"])
            pom {
                name.set("dengjen-tts")
                description.set("Java bindings for libdengjen, the C API for dengjen-tts.")
                url.set("https://github.com/ZirekHQ/dengjen")
                licenses {
                    license {
                        name.set("GPL-3.0-or-later")
                        url.set("https://www.gnu.org/licenses/gpl-3.0.txt")
                        distribution.set("repo")
                    }
                }
                scm {
                    connection.set("scm:git:https://github.com/ZirekHQ/dengjen.git")
                    developerConnection.set("scm:git:git@github.com:ZirekHQ/dengjen.git")
                    url.set("https://github.com/ZirekHQ/dengjen.git")
                }
                developers {
                    developer {
                        id.set("austek")
                        name.set("Ali Ustek")
                    }
                }
            }
        }
    }
    repositories {
        maven { url = uri(stagingDir.get()) }
    }
}

tasks.named("check") {
    dependsOn(testing.suites.named("integrationTest"))
    dependsOn(testing.suites.named("e2e"))
}

val nativeClassifiers = listOf("linux-x86_64", "linux-aarch64", "windows-x64", "macos-aarch64")



fun expectedNativeLibraryFileName(classifier: String): String =
    when (classifier) {
        "linux-x86_64", "linux-aarch64" -> "liblibdengjen.so"
        "windows-x64" -> "libdengjen.dll"
        "macos-aarch64" -> "liblibdengjen.dylib"
        else -> throw GradleException("unknown classifier: $classifier")
    }

val nativeArtifactsDir: Directory =
    layout.projectDirectory.dir((findProperty("nativeArtifactsDir") as String?) ?: "native-artifacts")

// The native libraries ship once per native change as their own artifact, one classifier jar per
// platform, so a binding-only release uploads a few KB. natives.fingerprint is regenerated with
// scripts/natives-fingerprint.sh --update; bump nativesVersion whenever it changes.
val nativesVersion = "1.0.7"
val nativesArtifactId = "dengjen-tts-natives"
val nativesFingerprint: String = file("natives.fingerprint").readText().trim()

val skipNativesPublish = providers.gradleProperty("skipNativesPublish").map(String::toBoolean).orElse(false)

val publishRequested =
    gradle.startParameter.taskNames.any { it.contains("publish", ignoreCase = true) || it.contains("jreleaser", ignoreCase = true) }

val nativeClassifierJars =
    nativeClassifiers.associateWith { classifier ->
        tasks.register<Jar>("nativeJar-$classifier") {
            archiveBaseName.set(nativesArtifactId)
            archiveVersion.set(nativesVersion)
            archiveClassifier.set(classifier)
            val sourceDir = nativeArtifactsDir.dir(classifier)
            from(sourceDir) { into("natives/$classifier") }
            onlyIf { !skipNativesPublish.get() && (sourceDir.asFile.exists() || publishRequested) }
            doFirst {
                val expectedName = expectedNativeLibraryFileName(classifier)
                val files = sourceDir.asFile.listFiles().orEmpty()
                check(files.size == 1 && files[0].isFile && files[0].name == expectedName) {
                    "expected exactly one file named '$expectedName' in $sourceDir, found ${files.toList()}"
                }
            }
        }
    }

// Maven Central requires sources and javadoc jars for every artifact; these carry a note only.
val nativesAuxiliaryJars =
    listOf("sources", "javadoc").map { kind ->
        tasks.register<Jar>("natives${kind.replaceFirstChar { it.uppercase() }}Jar") {
            archiveBaseName.set(nativesArtifactId)
            archiveVersion.set(nativesVersion)
            archiveClassifier.set(kind)
            from(resources.text.fromString("$nativesArtifactId ships binaries only; see https://github.com/ZirekHQ/dengjen.\n")) {
                rename { "README.txt" }
            }
        }
    }

publishing {
    publications {
        create<MavenPublication>("natives") {
            artifactId = nativesArtifactId
            version = nativesVersion
            nativeClassifierJars.values.forEach { jarTask -> artifact(jarTask) }
            nativesAuxiliaryJars.forEach { jarTask -> artifact(jarTask) }
            pom {
                name.set("dengjen-tts-natives")
                description.set("Native libdengjen libraries (one classifier per platform) for the dengjen-tts Java bindings.")
                url.set("https://github.com/ZirekHQ/dengjen")
                licenses {
                    license {
                        name.set("GPL-3.0-or-later")
                        url.set("https://www.gnu.org/licenses/gpl-3.0.txt")
                        distribution.set("repo")
                    }
                }
                scm {
                    connection.set("scm:git:https://github.com/ZirekHQ/dengjen.git")
                    developerConnection.set("scm:git:git@github.com:ZirekHQ/dengjen.git")
                    url.set("https://github.com/ZirekHQ/dengjen.git")
                }
                developers {
                    developer {
                        id.set("austek")
                        name.set("Ali Ustek")
                    }
                }
                // The release workflow compares this with natives.fingerprint to catch a missed version bump.
                properties.put("dengjen.natives.fingerprint", nativesFingerprint)
            }
        }
    }
}

tasks.withType<PublishToMavenRepository>().configureEach {
    onlyIf { !(publication.name == "natives" && skipNativesPublish.get()) }
}

tasks.withType<PublishToMavenLocal>().configureEach {
    onlyIf { !(publication.name == "natives" && skipNativesPublish.get()) }
}

configure<org.jreleaser.gradle.plugin.JReleaserExtension> {
    // Validates and logs everything (signing, Maven Central deploy, GitHub
    // Release) without performing the remote writes -- the Gradle plugin
    // doesn't pick up JRELEASER_DRY_RUN from the environment on its own,
    // unlike the CLI/Maven, so it's wired through explicitly here.
    dryrun = providers.environmentVariable("JRELEASER_DRY_RUN_OVERRIDE").orNull == "true"
    gitRootSearch = true
    release {
        github {


            skipTag = true




            tagName = "java-v{{projectVersion}}"




            // Overrides JReleaser's auto-detected changelog baseline (the
            // previous java-v* release) -- for testing/dry runs against an
            // arbitrary prior version. Unset means auto-detect, same as
            // before this override existed.
            providers.environmentVariable("JRELEASER_PREVIOUS_TAG_OVERRIDE").orNull
                ?.let { previousTagName = it }
            immutableRelease = true
            changelog {
                formatted = org.jreleaser.model.Active.ALWAYS
                preset = "conventional-commits"
                links = true
            }
        }
    }
    signing {
        pgp {
            active = org.jreleaser.model.Active.ALWAYS
            armored = true
        }
    }
    deploy {
        maven {
            mavenCentral {
                register("sonatype") {
                    active = org.jreleaser.model.Active.ALWAYS
                    url = "https://central.sonatype.com/api/v1/publisher"
                    stagingRepository(stagingDir.get().toString())
                }
            }
        }
    }
}
