package org.kpc.rescaleframecraft.client;

import java.util.function.Consumer;
import java.util.function.Supplier;
import net.caffeinemc.mods.sodium.api.config.ConfigEntryPoint;
import net.caffeinemc.mods.sodium.api.config.ConfigState;
import net.caffeinemc.mods.sodium.client.config.ConfigManager;
import net.caffeinemc.mods.sodium.api.config.structure.ConfigBuilder;
import net.caffeinemc.mods.sodium.api.config.structure.EnumOptionBuilder;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.ResourceLocation;

/** Sodium's supported config API supplies the entry in Reese's Sodium Options. */
public final class SodiumConfig implements ConfigEntryPoint {
    public static void capabilitiesChanged() {
        if(ConfigManager.CONFIG!=null)ConfigManager.CONFIG.invalidateGlobalRebuildDependents();
    }
    private static ResourceLocation id(String path) {return ResourceLocation.fromNamespaceAndPath("rescaleframecraft",path);}
    private static Component text(String value) {return Component.literal(value);}
    private <E extends Enum<E>> EnumOptionBuilder<E> choice(ConfigBuilder b,String id,String name,Class<E> type,E initial,Consumer<E> set,Supplier<E> get,String tip) {
        return b.createEnumOption(id(id),type).setName(text(name)).setTooltip(text(tip)).setDefaultValue(initial)
            .setElementNameProvider(value->text(value.name().replace('_',' '))).setBinding(set,get).setStorageHandler(TaaSettings::save);
    }
    @Override public void registerConfigLate(ConfigBuilder b) {
        b.registerOwnModOptions().addPage(b.createOptionPage().setName(text("ReScaleFrame"))
            .addOptionGroup(b.createOptionGroup().setName(text("Scene reconstruction"))
                .addOption(b.createBooleanOption(id("taa")).setName(text("Stable TAA")).setDefaultValue(true)
                    .setTooltip(text("Eight raw scene samples. Used when the upscaler is off. Hand and HUD render afterwards."))
                    .setBinding(value->TaaSettings.current.enabled=value,()->TaaSettings.current.enabled).setStorageHandler(TaaSettings::save))
                .addOption(choice(b,"upscaler","Upscaler",TaaSettings.Upscaler.class,TaaSettings.Upscaler.OFF,
                    value->TaaSettings.current.upscaler=value,()->TaaSettings.current.upscaler,
                    "Reconstructs a smaller world image at display resolution. Native uses temporal AA at full resolution. Applies without restarting. F3 shows the actual backend and errors."))
                .addOption(choice(b,"dlss_preset","DLSS model preset",TaaSettings.DlssPreset.class,TaaSettings.DlssPreset.DEFAULT,
                    value->TaaSettings.current.dlssPreset=value,()->TaaSettings.current.dlssPreset,
                    "DLSS model selection, independent of Quality. Default lets the SDK choose; E/F are legacy models. Applies without restarting. NVIDIA driver overrides may replace this request."))
                .addOption(choice(b,"quality","Quality",TaaSettings.Quality.class,TaaSettings.Quality.QUALITY,
                    value->TaaSettings.current.quality=value,()->TaaSettings.current.quality,
                    "The selected SDK chooses the render size. Native is full resolution; Performance renders fewer pixels.")))
            .addOptionGroup(b.createOptionGroup().setName(text("Frame generation and latency"))
                .addOption(choice(b,"generation","Frame generation",TaaSettings.FrameGeneration.class,TaaSettings.FrameGeneration.OFF,
                    value->TaaSettings.current.frameGeneration=value,()->TaaSettings.current.frameGeneration,
                    "Independent of the upscaler. Requires VSync off. Menus, loading and reset frames suspend interpolation. Unsupported hardware or runtimes are reported in F3."))
                .addOption(b.createIntegerOption(id("generated_frames")).setName(text("Frame generation multiplier")).setDefaultValue(1)
                    .setRangeProvider(state->FrameGenerationLimits.INSTANCE.range(state.readEnumOption(id("generation"),TaaSettings.FrameGeneration.class)),id("generation"),ConfigState.UPDATE_ON_REBUILD)
                    .setEnabledProvider(state->{var fg=state.readEnumOption(id("generation"),TaaSettings.FrameGeneration.class);return fg!=TaaSettings.FrameGeneration.OFF&&FrameGenerationLimits.INSTANCE.maximum(fg)>1;},id("generation"),ConfigState.UPDATE_ON_REBUILD)
                    .setControlHiddenWhenDisabled(false).setValueFormatter(value->text((value+1)+"x"))
                    .setTooltip(text("Total output multiplier, including the rendered frame. Available values depend on the selected frame-generation technology and GPU. Apply the selected technology once to detect its supported range."))
                    .setBinding(value->TaaSettings.current.generatedFrames=value,()->TaaSettings.current.generatedFrames).setStorageHandler(TaaSettings::save))
                .addOption(choice(b,"reflex","Low latency",TaaSettings.Reflex.class,TaaSettings.Reflex.ON,
                    value->TaaSettings.current.reflex=value,()->TaaSettings.current.reflex,
                    "Reflex on supported Streamline paths; XeLL pacing on supported XeSS paths. Boost requests higher GPU clocks while rendering."))
                .addOption(b.createIntegerOption(id("frame_limit")).setName(text("Rendered FPS limit")).setDefaultValue(0).setRange(0,240,1)
                    .setValueFormatter(value->text(value==0?"Unlimited":value+" FPS"))
                    .setTooltip(text("Zero is unlimited. Caps real frames before generation. Minecraft's own cap still applies."))
                    .setBinding(value->TaaSettings.current.frameLimit=value,()->TaaSettings.current.frameLimit).setStorageHandler(TaaSettings::save)))
            .addOptionGroup(b.createOptionGroup().setName(text("Sharpening"))
                .addOption(choice(b,"sharpening_mode","Mode",TaaSettings.Sharpener.class,TaaSettings.Sharpener.OFF,
                    value->TaaSettings.current.sharpeningMode=value,()->TaaSettings.current.sharpeningMode,
                    "Applies live after native temporal upscaling. Vendor mode requires an active FSR backend. Depth modes fall back to RCAS if depth is unavailable."))
                .addOption(b.createIntegerOption(id("sharpening_sharpness")).setName(text("Sharpness")).setDefaultValue(400)
                    .setRange(0,1000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setTooltip(text("Base sharpening strength. Zero with MAS off bypasses the pass."))
                    .setBinding(value->TaaSettings.current.sharpening_sharpness=value,()->TaaSettings.current.sharpening_sharpness).setStorageHandler(TaaSettings::save))
                .addOption(b.createBooleanOption(id("sharpening_mas")).setName(text("Motion adaptive sharpening")).setDefaultValue(false)
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Raises sharpness with motion above the threshold."))
                    .setBinding(value->TaaSettings.current.sharpening_mas=value,()->TaaSettings.current.sharpening_mas).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_motion_sharpness")).setName(text("Motion sharpness")).setDefaultValue(200)
                    .setRange(-1000,1000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Additional sharpness at the motion scale limit."))
                    .setBinding(value->TaaSettings.current.sharpening_motion_sharpness=value,()->TaaSettings.current.sharpening_motion_sharpness).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_motion_threshold")).setName(text("Motion threshold (pixels)")).setDefaultValue(0)
                    .setRange(0,100000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Motion below this speed keeps base sharpness."))
                    .setBinding(value->TaaSettings.current.sharpening_motion_threshold=value,()->TaaSettings.current.sharpening_motion_threshold).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_motion_scale_limit")).setName(text("Motion scale limit (pixels)")).setDefaultValue(10000)
                    .setRange(10,100000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Motion speed where the full motion amount applies."))
                    .setBinding(value->TaaSettings.current.sharpening_motion_scale_limit=value,()->TaaSettings.current.sharpening_motion_scale_limit).setStorageHandler(TaaSettings::save))
                .addOption(b.createBooleanOption(id("sharpening_contrast_enabled")).setName(text("Contrast enabled")).setDefaultValue(false)
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Enable additional local contrast."))
                    .setBinding(value->TaaSettings.current.sharpening_contrast_enabled=value,()->TaaSettings.current.sharpening_contrast_enabled).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_contrast")).setName(text("Contrast amount")).setDefaultValue(0)
                    .setRange(0,2000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.RCAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("RCAS only. Additional contrast from zero to two."))
                    .setBinding(value->TaaSettings.current.sharpening_contrast=value,()->TaaSettings.current.sharpening_contrast).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_depth_scale")).setName(text("Depth scale")).setDefaultValue(35000)
                    .setRange(0,600000,10).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.3f",value/1000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.DEPTH_RCAS||mode==TaaSettings.Sharpener.DEPTH_DAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("Depth modes only. Scales depth separation to reject edges."))
                    .setBinding(value->TaaSettings.current.sharpening_depth_scale=value,()->TaaSettings.current.sharpening_depth_scale).setStorageHandler(TaaSettings::save))
                .addOption(b.createIntegerOption(id("sharpening_depth_bias")).setName(text("Depth bias")).setDefaultValue(1000)
                    .setRange(0,1000000,1).setValueFormatter(value->text(String.format(java.util.Locale.ROOT,"%.6f",value/1000000.0)))
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.DEPTH_RCAS||mode==TaaSettings.Sharpener.DEPTH_DAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("Depth modes only. Depth tolerance before rejection."))
                    .setBinding(value->TaaSettings.current.sharpening_depth_bias=value,()->TaaSettings.current.sharpening_depth_bias).setStorageHandler(TaaSettings::save))
                .addOption(b.createBooleanOption(id("sharpening_clamp_output")).setName(text("Clamp depth output")).setDefaultValue(false)
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode==TaaSettings.Sharpener.DEPTH_RCAS||mode==TaaSettings.Sharpener.DEPTH_DAS;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("Depth modes only. Clamp the sharpened output."))
                    .setBinding(value->TaaSettings.current.sharpening_clamp_output=value,()->TaaSettings.current.sharpening_clamp_output).setStorageHandler(TaaSettings::save))
                .addOption(b.createBooleanOption(id("sharpening_debug")).setName(text("Debug tint")).setDefaultValue(false)
                    .setEnabledProvider(state->{var mode=state.readEnumOption(id("sharpening_mode"),TaaSettings.Sharpener.class);return mode!=TaaSettings.Sharpener.OFF&&mode!=TaaSettings.Sharpener.VENDOR_FSR_ONLY;},id("sharpening_mode"),ConfigState.UPDATE_ON_REBUILD).setControlHiddenWhenDisabled(false)
                    .setTooltip(text("Show motion or depth rejection tint."))
                    .setBinding(value->TaaSettings.current.sharpening_debug=value,()->TaaSettings.current.sharpening_debug).setStorageHandler(TaaSettings::save)))
            .addOptionGroup(b.createOptionGroup().setName(text("Diagnostics"))
                .addOption(b.createBooleanOption(id("native_debug")).setName(text("Native diagnostics")).setDefaultValue(false)
                    .setTooltip(text("Write native diagnostics under .minecraft/rescaleframe. F3 displays requested and active providers."))
                    .setBinding(value->TaaSettings.current.nativeDebug=value,()->TaaSettings.current.nativeDebug).setStorageHandler(TaaSettings::save))));
    }
}
