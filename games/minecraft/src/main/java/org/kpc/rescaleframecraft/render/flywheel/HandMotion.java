package org.kpc.rescaleframecraft.render.flywheel;

import com.mojang.blaze3d.vertex.MeshData;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.RenderType;
import net.minecraft.world.item.ItemStack;
import org.kpc.rescaleframecraft.render.TemporalRenderer;

/** Tracks Iris hand batches separately from actors, including the pack's final vertex deformation. */
public final class HandMotion {
    private static final Map<RenderType,Segments> solid=new IdentityHashMap<>(),translucent=new IdentityHashMap<>();
    private static Map<RenderType,Segments> pass;
    private static ItemStack main=ItemStack.EMPTY,off=ItemStack.EMPTY;
    private static Object player,topology=new Object();
    private HandMotion() {}
    private static final class Segments {
        final ArrayList<Object> owners=new ArrayList<>();
        int used;
        Object next() {if(used==owners.size())owners.add(new Object());return owners.get(used++);}
    }
    public static void beginFrame() {
        solid.values().forEach(value->value.used=0);translucent.values().forEach(value->value.used=0);
        var current=Minecraft.getInstance().player;
        if(current==null)return;
        if(player!=current||!ItemStack.isSameItemSameComponents(main,current.getMainHandItem())
            ||!ItemStack.isSameItemSameComponents(off,current.getOffhandItem())) {
            topology=new Object();player=current;
            main=current.getMainHandItem().copy();off=current.getOffhandItem().copy();
        }
    }
    public static void render(boolean opaque,Runnable original) {
        var saved=pass;
        pass=TemporalRenderer.capturing()?(opaque?solid:translucent):null;
        try {original.run();} finally {pass=saved;}
    }
    public static void draw(RenderType type,MeshData mesh,Runnable original) {
        if(pass==null||!TemporalRenderer.capturing()) {original.run();return;}
        Object owner=pass.computeIfAbsent(type,key->new Segments()).next();
        GpuMotion.immediate(owner,topology,mesh.drawState().vertexCount(),original);
    }
    public static void endFrame() {prune(solid);prune(translucent);}
    private static void prune(Map<RenderType,Segments> batches) {
        batches.values().removeIf(value->{
            value.owners.subList(value.used,value.owners.size()).clear();return value.used==0;
        });
    }
    public static void clear() {solid.clear();translucent.clear();pass=null;main=off=ItemStack.EMPTY;player=null;topology=new Object();}
}
