package org.kpc.rescaleframecraft.client;

import net.caffeinemc.mods.sodium.client.config.ConfigManager;
import net.caffeinemc.mods.sodium.client.config.builder.ConfigBuilderImpl;
import net.caffeinemc.mods.sodium.client.config.structure.Config;
import net.minecraft.resources.ResourceLocation;
import java.util.ArrayList;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SodiumConfigTest {
    @Test void legacySharpeningSurvivesJsonMigrationInput() {
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
    }
    @Test void actualSodiumBuilderAcceptsAllRegisteredOptions() {
        var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","1.1.0-dev"),"rescaleframecraft");
        assertDoesNotThrow(()->new SodiumConfig().registerConfigLate(builder));
        var configs=builder.build();assertEquals(1,configs.size());
        assertEquals(1,configs.iterator().next().pages().size());
        var config=new Config(new ArrayList<>(configs));
        assertNotNull(config.getOption(ResourceLocation.parse("rescaleframecraft:taa")));
    }
}
