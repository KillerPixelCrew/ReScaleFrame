package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.MeshData;
import org.kpc.rescaleframecraft.render.flywheel.ActorMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(BufferBuilder.class)
abstract class ActorBufferBuilderMixin {
    @Shadow private int vertices;
    @Unique private ActorMotion.Stream rfc$vertices;
    @Inject(method="beginVertex",at=@At("RETURN"))
    private void rfc$vertex(CallbackInfoReturnable<Long> cir) {rfc$vertices=ActorMotion.vertex(rfc$vertices,vertices);}
    @Inject(method="build",at=@At("RETURN"))
    private void rfc$mesh(CallbackInfoReturnable<MeshData> cir) {ActorMotion.mesh(rfc$vertices,cir.getReturnValue(),vertices);rfc$vertices=null;}
}
