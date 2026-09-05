# WSGM integration

This directory will contain the ReScaleFrame side of WSGM launch/profile integration. The same loader and runtime must remain usable through the standalone application.

Use WSGM's canonical application identity and coordinate both managed launch and native Steam Input lease paths. WSGM owns profile persistence in a managed session. The in-game runtime owns GPU work and reports requested versus effective capabilities.

Frame limiting and AutoTDP need rendered-frame timing, separately from generated/presented FPS. No WSGM changes are included in this scaffold.
