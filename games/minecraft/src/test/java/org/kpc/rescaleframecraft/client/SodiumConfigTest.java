package org.kpc.rescaleframecraft.client;

import net.caffeinemc.mods.sodium.client.config.ConfigManager;
import net.caffeinemc.mods.sodium.client.config.builder.ConfigBuilderImpl;
import net.caffeinemc.mods.sodium.client.config.structure.Config;
import net.caffeinemc.mods.sodium.client.config.structure.EnumOption;
import net.caffeinemc.mods.sodium.client.config.structure.IntegerOption;
import net.minecraft.resources.ResourceLocation;
import java.util.ArrayList;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SodiumConfigTest {
    @Test void multiplierRangeReactsToThePendingFgChoiceAndNativeCapabilityChanges() {
        var saved=TaaSettings.current;var limits=FrameGenerationLimits.INSTANCE;limits.reset();
        TaaSettings.current=new TaaSettings();TaaSettings.current.frameGeneration=TaaSettings.FrameGeneration.DLSS;
        try {
            var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","test"),"rescaleframecraft");
            new SodiumConfig().registerConfigLate(builder);
            var config=new Config(new ArrayList<>(builder.build()));
            var count=(IntegerOption)config.getOption(ResourceLocation.parse("rescaleframecraft:generated_frames"));
            @SuppressWarnings("unchecked") var fg=(EnumOption<TaaSettings.FrameGeneration>)config.getOption(ResourceLocation.parse("rescaleframecraft:generation"));
            assertEquals(1,count.getSteppedValidator().max());assertFalse(count.isEnabled());
            limits.record(1,1,5,true);config.invalidateGlobalRebuildDependents();
            assertTrue(count.isEnabled());assertEquals(5,count.getSteppedValidator().max());
            count.modifyValue(5);assertEquals("6x",count.formatValue(count.getValidatedValue()).getString());
            fg.modifyValue(TaaSettings.FrameGeneration.FSR_3);
            assertEquals(1,count.getSteppedValidator().max());assertEquals(1,count.getValidatedValue());assertFalse(count.isEnabled());
            fg.modifyValue(TaaSettings.FrameGeneration.DLSS);count.modifyValue(5);
            limits.record(1,1,3,true);config.invalidateGlobalRebuildDependents();
            assertEquals(3,count.getValidatedValue());assertEquals("4x",count.formatValue(count.getValidatedValue()).getString());
            fg.modifyValue(TaaSettings.FrameGeneration.OFF);assertFalse(count.isEnabled());
        } finally {TaaSettings.current=saved;limits.reset();}
    }
    @Test void actualSodiumBuilderAcceptsAllRegisteredOptions() {
        var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","1.1.0-dev"),"rescaleframecraft");
        assertDoesNotThrow(()->new SodiumConfig().registerConfigLate(builder));
        var configs=builder.build();assertEquals(1,configs.size());
        assertEquals(1,configs.iterator().next().pages().size());
    }
}
