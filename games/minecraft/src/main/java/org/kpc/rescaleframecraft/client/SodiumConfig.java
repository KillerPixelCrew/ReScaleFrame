package org.kpc.rescaleframecraft.client;

import java.util.function.Consumer;
import java.util.function.Supplier;
import net.caffeinemc.mods.sodium.api.config.ConfigEntryPoint;
import net.caffeinemc.mods.sodium.api.config.structure.ConfigBuilder;
import net.caffeinemc.mods.sodium.api.config.structure.EnumOptionBuilder;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.ResourceLocation;

/** Sodium's supported config API supplies the entry in Reese's Sodium Options. */
public final class SodiumConfig implements ConfigEntryPoint {
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
                .addOption(b.createIntegerOption(id("generated_frames")).setName(text("Generated frames per rendered frame")).setDefaultValue(1).setRange(1,15,1)
                    .setValueFormatter(value->text(Integer.toString(value)))
                    .setTooltip(text("Clamped to the active provider and GPU capability. One generated frame doubles the requested output rate."))
                    .setBinding(value->TaaSettings.current.generatedFrames=value,()->TaaSettings.current.generatedFrames).setStorageHandler(TaaSettings::save))
                .addOption(choice(b,"reflex","Low latency",TaaSettings.Reflex.class,TaaSettings.Reflex.ON,
                    value->TaaSettings.current.reflex=value,()->TaaSettings.current.reflex,
                    "Reflex on supported Streamline paths; XeLL pacing on supported XeSS paths. Boost requests higher GPU clocks while rendering."))
                .addOption(b.createIntegerOption(id("frame_limit")).setName(text("Rendered FPS limit")).setDefaultValue(0).setRange(0,240,1)
                    .setValueFormatter(value->text(value==0?"Unlimited":value+" FPS"))
                    .setTooltip(text("Zero is unlimited. Caps real frames before generation. Minecraft's own cap still applies."))
                    .setBinding(value->TaaSettings.current.frameLimit=value,()->TaaSettings.current.frameLimit).setStorageHandler(TaaSettings::save)))
            .addOptionGroup(b.createOptionGroup().setName(text("Diagnostics"))
                .addOption(b.createBooleanOption(id("native_debug")).setName(text("Native diagnostics")).setDefaultValue(false)
                    .setTooltip(text("Write native diagnostics under .minecraft/rescaleframe. F3 displays requested and active providers."))
                    .setBinding(value->TaaSettings.current.nativeDebug=value,()->TaaSettings.current.nativeDebug).setStorageHandler(TaaSettings::save))));
    }
}
