pipeline {
    agent any
    options {
        buildDiscarder(logRotator(numToKeepStr: "10", artifactNumToKeepStr: "1"))
    }
    stages {
        stage("Test") {
            steps {
                script {
                    docker.image("rust:1-trixie").inside {
                        sh "cargo test -p fdrive-core"
                    }
                }
            }
        }
        stage("Build") {
            steps {
                script {
                    docker.image("rust:1-trixie").inside("--user=root") {
                        sh "apt-get update && apt-get install -y libgtk-3-dev libayatana-appindicator3-dev"
                        sh "cargo build --release -p fdrive-gtk"
                        sh "cargo build --release -p fdrive-linux"
                    }
                    docker.image("rust:1-trixie").inside("--user=root") {
                        sh "apt-get update && apt-get install -y gcc-mingw-w64-x86-64"
                        sh "rustup target add x86_64-pc-windows-gnu"
                        sh "cargo build --release --target x86_64-pc-windows-gnu -p fdrive-windows"
                    }
                    docker.image("rust:1-trixie").inside("--user=root") {
                        sh "apt-get update && apt-get install -y openjdk-21-jdk-headless unzip"
                        sh "mkdir -p /opt/android-sdk/cmdline-tools && curl -sSL https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip -o /tmp/tools.zip && unzip -q /tmp/tools.zip -d /opt/android-sdk/cmdline-tools && mv /opt/android-sdk/cmdline-tools/cmdline-tools /opt/android-sdk/cmdline-tools/latest"
                        sh "yes | /opt/android-sdk/cmdline-tools/latest/bin/sdkmanager --licenses > /dev/null"
                        sh "/opt/android-sdk/cmdline-tools/latest/bin/sdkmanager 'platform-tools' 'platforms;android-35' 'build-tools;35.0.0' 'ndk;27.2.12479018'"
                        sh "rustup target add aarch64-linux-android x86_64-linux-android && cargo install cargo-ndk"
                        sh "cd crates/fdrive-android/android && ANDROID_HOME=/opt/android-sdk ./gradlew assembleDebug"
                    }
                }
            }
        }
        stage("Release") {
            steps {
                script {
                    docker.image("alpine").inside("--user=root") {
                        withCredentials([sshUserPrivateKey(credentialsId: "app-filestash-hal", keyFileVariable: "SSH_KEY", usernameVariable: "SSH_USER")]) {
                            sh "apk add openssh-client"
                            sh 'scp -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no target/release/fdrive-gtk "$SSH_USER@hal.filestash.app:/mnt/me-kerjean-pages/projects/filestash/downloads/latest/fdrive-linux-gtk-x86_64.bin"'
                            sh 'scp -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no target/release/fdrive "$SSH_USER@hal.filestash.app:/mnt/me-kerjean-pages/projects/filestash/downloads/latest/fdrive-linux-x86_64.bin"'
                            sh 'scp -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no target/x86_64-pc-windows-gnu/release/fdrive-windows.exe "$SSH_USER@hal.filestash.app:/mnt/me-kerjean-pages/projects/filestash/downloads/latest/fdrive-windows-x86_64.exe"'
                            sh 'scp -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=no crates/fdrive-android/android/app/build/outputs/apk/debug/app-debug.apk "$SSH_USER@hal.filestash.app:/mnt/me-kerjean-pages/projects/filestash/downloads/latest/fdrive-android.apk"'
                        }
                    }
                }
            }
        }
    }
    post {
        always {
            script {
                def owner = sh(script: 'echo "$(id -u):$(id -g)"', returnStdout: true).trim()
                docker.image("alpine").inside("--user=root") {
                    sh "chown -R ${owner} ."
                }
            }
            cleanWs(disableDeferredWipeout: true, deleteDirs: true)
        }
    }
}
