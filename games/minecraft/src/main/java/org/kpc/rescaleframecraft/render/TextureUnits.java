package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.platform.GlStateManager;
import com.mojang.blaze3d.systems.RenderSystem;
import org.lwjgl.opengl.*;

final class TextureUnits {
    private TextureUnits() {}
    static void bind(int unit,int texture) {
        if(unit<12) {
            RenderSystem.activeTexture(GL13.GL_TEXTURE0+unit);RenderSystem.bindTexture(texture);
        } else {
            // Minecraft only caches twelve units; the upstream cache needs sixteen.
            GL13.glActiveTexture(GL13.GL_TEXTURE0+unit);GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
            GL13.glActiveTexture(GlStateManager._getActiveTexture());
        }
    }
}
