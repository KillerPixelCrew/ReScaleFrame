package org.kpc.rescaleframecraft.client;

import net.caffeinemc.mods.sodium.api.config.ConfigEntryPoint;
import net.caffeinemc.mods.sodium.api.config.structure.ConfigBuilder;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.ResourceLocation;

/** Sodium owns the game-specific TAA toggle. Insert opens the shared native settings. */
public final class SodiumConfig implements ConfigEntryPoint {
    @Override public void registerConfigLate(ConfigBuilder builder) {
        var taa=ResourceLocation.fromNamespaceAndPath("rescaleframecraft","taa");
        builder.registerOwnModOptions().addPage(builder.createOptionPage()
            .setName(Component.literal("ReScaleFrame"))
            .addOptionGroup(builder.createOptionGroup().setName(Component.literal("Scene reconstruction"))
                .addOption(builder.createBooleanOption(taa).setName(Component.literal("Stable TAA"))
                    .setDefaultValue(true)
                    .setTooltip(Component.literal("Eight raw scene samples when native upscaling is off. Insert opens the shared native settings."))
                    .setBinding(value->TaaSettings.current.enabled=value,()->TaaSettings.current.enabled)
                    .setStorageHandler(TaaSettings::save))));
    }
}
