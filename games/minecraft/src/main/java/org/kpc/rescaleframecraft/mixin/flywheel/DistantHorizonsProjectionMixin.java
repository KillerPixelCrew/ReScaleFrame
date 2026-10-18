package org.kpc.rescaleframecraft.mixin.flywheel;

import org.joml.Matrix4fc;
import org.kpc.rescaleframecraft.render.DistantHorizonsIntegration;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyVariable;

@Pseudo
@Mixin(targets="net.irisshaders.iris.compat.dh.IrisLodRenderProgram",remap=false)
abstract class DistantHorizonsProjectionMixin {
    @ModifyVariable(method="fillUniformData",at=@At("HEAD"),argsOnly=true,ordinal=0)
    private Matrix4fc rfc$sceneJitter(Matrix4fc projection) {return DistantHorizonsIntegration.lodProjection(projection);}
}
