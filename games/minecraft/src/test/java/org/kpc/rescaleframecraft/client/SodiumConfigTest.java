package org.kpc.rescaleframecraft.client;

import net.caffeinemc.mods.sodium.client.config.ConfigManager;
import net.caffeinemc.mods.sodium.client.config.builder.ConfigBuilderImpl;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SodiumConfigTest {
    @Test void actualSodiumBuilderAcceptsAllRegisteredOptions() {
        var builder=new ConfigBuilderImpl(id->new ConfigManager.ModMetadata("ReScaleFrameCraft","1.1.0-dev"),"rescaleframecraft");
        assertDoesNotThrow(()->new SodiumConfig().registerConfigLate(builder));
        var configs=builder.build();assertEquals(1,configs.size());
        assertEquals(1,configs.iterator().next().pages().size());
    }
}
