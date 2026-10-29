# dengjen-speechd

Speech Dispatcher output module for the dengjen speech synthesis engine.

Builds the `sd_dengjen` binary. Speech Dispatcher starts it as a child process and talks to it over stdin and stdout, so Linux applications and screen readers that speak through Speech Dispatcher can use any installed Piper voice, including Kurmanji.

Build, install and configure it with the Speech Dispatcher module guide in the documentation.
