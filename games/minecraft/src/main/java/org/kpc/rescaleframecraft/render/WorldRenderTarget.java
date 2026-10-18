package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.TextureTarget;

/** Identifies the owned scene allocation for the sized-depth allocation hook. */
public final class WorldRenderTarget extends TextureTarget {
    public WorldRenderTarget(int width,int height,boolean clearError) {super(width,height,true,clearError);}
}
