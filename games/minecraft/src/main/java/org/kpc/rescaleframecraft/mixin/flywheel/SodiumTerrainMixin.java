package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.llamalad7.mixinextras.sugar.Local;
import net.caffeinemc.mods.sodium.client.gl.device.CommandList;
import net.caffeinemc.mods.sodium.client.gl.device.DrawCommandList;
import net.caffeinemc.mods.sodium.client.gl.device.MultiDrawBatch;
import net.caffeinemc.mods.sodium.client.gl.tessellation.GlIndexType;
import net.caffeinemc.mods.sodium.client.gl.tessellation.GlTessellation;
import net.caffeinemc.mods.sodium.client.render.chunk.DefaultChunkRenderer;
import net.caffeinemc.mods.sodium.client.render.chunk.data.SectionRenderDataStorage;
import net.caffeinemc.mods.sodium.client.render.chunk.lists.ChunkRenderList;
import org.kpc.rescaleframecraft.render.flywheel.GpuMotion;
import org.kpc.rescaleframecraft.render.flywheel.TerrainMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

@Mixin(value=DefaultChunkRenderer.class,remap=false)
abstract class SodiumTerrainMixin {
    @WrapOperation(method="render",at=@At(value="INVOKE",target="Lnet/caffeinemc/mods/sodium/client/render/chunk/DefaultChunkRenderer;executeDrawBatch(Lnet/caffeinemc/mods/sodium/client/gl/device/CommandList;Lnet/caffeinemc/mods/sodium/client/gl/tessellation/GlTessellation;Lnet/caffeinemc/mods/sodium/client/gl/device/MultiDrawBatch;)V"))
    private void rfc$region(CommandList commands,GlTessellation tessellation,MultiDrawBatch batch,Operation<Void> original,
                                   @Local SectionRenderDataStorage storage,@Local ChunkRenderList list) {
        TerrainMotion.render(storage,list,()->original.call(commands,tessellation,batch));
    }
    @WrapOperation(method="executeDrawBatch",at=@At(value="INVOKE",target="Lnet/caffeinemc/mods/sodium/client/gl/device/DrawCommandList;multiDrawElementsBaseVertex(Lnet/caffeinemc/mods/sodium/client/gl/device/MultiDrawBatch;Lnet/caffeinemc/mods/sodium/client/gl/tessellation/GlIndexType;)V"))
    private static void rfc$draw(DrawCommandList commands,MultiDrawBatch batch,GlIndexType type,Operation<Void> original) {
        GpuMotion.drawImmediate(()->original.call(commands,batch,type));
    }
}
