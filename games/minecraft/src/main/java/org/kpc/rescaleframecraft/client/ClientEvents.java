package org.kpc.rescaleframecraft.client;

import com.mojang.blaze3d.platform.InputConstants;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.neoforged.neoforge.client.event.ClientTickEvent;
import net.neoforged.neoforge.client.event.ClientPlayerNetworkEvent;
import net.neoforged.neoforge.client.event.RenderFrameEvent;
import net.neoforged.neoforge.client.event.CustomizeGuiOverlayEvent;
import net.neoforged.neoforge.client.event.RegisterKeyMappingsEvent;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.kpc.rescaleframecraft.render.IrisIntegration;
import net.neoforged.fml.ModList;
import org.lwjgl.glfw.GLFW;

public final class ClientEvents {
    private static final KeyMapping OPTIONS=new KeyMapping("key.rescaleframecraft.options",InputConstants.Type.KEYSYM,GLFW.GLFW_KEY_F8,"key.categories.video");
    public static void keys(RegisterKeyMappingsEvent event) { event.register(OPTIONS); }
    public static void logout(ClientPlayerNetworkEvent.LoggingOut event) { TemporalRenderer.release(); }
    public static void frame(RenderFrameEvent.Post event) {
        TemporalRenderer.preflight();
        if(ModList.get().isLoaded("iris"))IrisIntegration.frame();
    }
    public static void tick(ClientTickEvent.Post event) {
        while(OPTIONS.consumeClick()) {
            var game=Minecraft.getInstance();game.setScreen(new TaaScreen(game.screen));
        }
    }
    public static void debug(CustomizeGuiOverlayEvent.DebugText event) { event.getLeft().add("ReScaleFrameCraft: "+TemporalRenderer.status()); }
}
