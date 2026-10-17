package org.kpc.rescaleframecraft;

import org.slf4j.Logger;

import com.mojang.logging.LogUtils;

import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.ModContainer;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.client.gui.IConfigScreenFactory;
import org.kpc.rescaleframecraft.client.ClientEvents;
import org.kpc.rescaleframecraft.client.TaaScreen;
import org.kpc.rescaleframecraft.client.TaaSettings;

// The value here should match an entry in the META-INF/neoforge.mods.toml file
@Mod(value=ReScaleFrameCraft.MODID,dist=Dist.CLIENT)
public class ReScaleFrameCraft {
    // Define mod id in a common place for everything to reference
    public static final String MODID = "rescaleframecraft";
    // Directly reference a slf4j logger
    public static final Logger LOGGER = LogUtils.getLogger();

    // The constructor for the mod class is the first code that is run when your mod is loaded.
    // FML will recognize some parameter types like IEventBus or ModContainer and pass them in automatically.
    public ReScaleFrameCraft(IEventBus modEventBus, ModContainer modContainer) {
        TaaSettings.load();
        modEventBus.addListener(ClientEvents::keys);
        NeoForge.EVENT_BUS.addListener(ClientEvents::tick);
        NeoForge.EVENT_BUS.addListener(ClientEvents::debug);
        NeoForge.EVENT_BUS.addListener(ClientEvents::logout);
        NeoForge.EVENT_BUS.addListener(ClientEvents::frame);
        modContainer.registerExtensionPoint(IConfigScreenFactory.class,(container,parent)->new TaaScreen(parent));
    }
}
