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
    @Test void sharpeningControlsFollowThePendingModeAndValuesSurviveJson() {
        var saved=TaaSettings.current;TaaSettings.current=new TaaSettings();
        try {
            var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","test"),"rescaleframecraft");
            new SodiumConfig().registerConfigLate(builder);var config=new Config(new ArrayList<>(builder.build()));
            @SuppressWarnings("unchecked") var mode=(EnumOption<TaaSettings.Sharpener>)config.getOption(ResourceLocation.parse("rescaleframecraft:sharpening_mode"));
            var motion=config.getOption(ResourceLocation.parse("rescaleframecraft:sharpening_motion_sharpness"));
            var depth=config.getOption(ResourceLocation.parse("rescaleframecraft:sharpening_depth_scale"));
            assertFalse(motion.isEnabled());assertFalse(depth.isEnabled());
            mode.modifyValue(TaaSettings.Sharpener.RCAS);assertTrue(motion.isEnabled());assertFalse(depth.isEnabled());
            mode.modifyValue(TaaSettings.Sharpener.DEPTH_DAS);assertFalse(motion.isEnabled());assertTrue(depth.isEnabled());
            mode.modifyValue(TaaSettings.Sharpener.VENDOR_FSR_ONLY);assertFalse(motion.isEnabled());assertFalse(depth.isEnabled());
            var value=new TaaSettings();value.sharpeningMode=TaaSettings.Sharpener.DEPTH_RCAS;value.sharpening_mas=true;
            value.sharpening_sharpness=713;value.sharpening_motion_sharpness=-310;value.sharpening_motion_threshold=4000;
            value.sharpening_motion_scale_limit=19000;value.sharpening_contrast_enabled=true;value.sharpening_contrast=1600;
            value.sharpening_depth_scale=128000;value.sharpening_depth_bias=3700;value.sharpening_clamp_output=true;value.sharpening_debug=true;
            var gson=new com.google.gson.Gson();var actual=gson.fromJson(gson.toJson(value),TaaSettings.class);
            assertArrayEquals(value.sharpening(),actual.sharpening());assertEquals(value.sharpeningState(),actual.sharpeningState());assertTrue(value.sharpeningState().matches(actual));
            actual.sharpening_sharpness++;assertFalse(value.sharpeningState().matches(actual));
            actual.sharpening_sharpness=3000;actual.sharpening_depth_bias=-1;actual.sanitizeSharpening();
            assertEquals(1000,actual.sharpening_sharpness);assertEquals(0,actual.sharpening_depth_bias);
            assertArrayEquals(new float[]{0,.4f,0,.2f,0,10,0,0,35,.001f,0,0},new TaaSettings().sharpening());
        } finally {TaaSettings.current=saved;}
    }
    @Test void actualSodiumBuilderAcceptsAllRegisteredOptions() {
        var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","1.1.0-dev"),"rescaleframecraft");
        assertDoesNotThrow(()->new SodiumConfig().registerConfigLate(builder));
        var configs=builder.build();assertEquals(1,configs.size());
        assertEquals(1,configs.iterator().next().pages().size());
    }
}
