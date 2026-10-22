# WEX Flutter App

单 Flutter 工程；Android 优先。当前为空间/对话/设置空态壳，没有真实 AI、GitHub 或 Dart→Rust 调用。
工具链：Flutter 3.35.7，Rust 1.85.1，JDK17，NDK27.0.12077973，Android SDK36，min API24；Gradle8.12，AGP8.9.1，Kotlin2.1.0。
pubspec.lock 由同版 Flutter 在 CI 生成并归档；后续 bootstrap 使用 enforce-lockfile。
运行前在仓库根执行 bash tools/bootstrap-flutter.sh，恢复官方 wrapper jar 和默认资源，再由 tools/build-android-native.sh 编译所需ABI。
APK 编译不需要模拟器；库加载和实际原生调用须另行运行验证。
未发送草稿仅在当前App进程中保留，未实现会话持久化。
