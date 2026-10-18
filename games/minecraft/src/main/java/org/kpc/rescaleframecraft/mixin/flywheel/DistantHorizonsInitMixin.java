package org.kpc.rescaleframecraft.mixin.flywheel;

import java.lang.reflect.Constructor;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import net.irisshaders.iris.compat.dh.DHCompatInternal;
import net.neoforged.fml.ModList;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.render.DistantHorizonsIntegration;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Pseudo
@Mixin(targets="net.irisshaders.iris.compat.dh.DHCompat",remap=false)
abstract class DistantHorizonsInitMixin {
    @Inject(method="getProjection",at=@At("RETURN"),cancellable=true)
    private static void rfc$samplingProjection(CallbackInfoReturnable<Matrix4f> cir) {
        if(ModList.get().isLoaded("distanthorizons")&&TemporalRenderer.capturing())
            cir.setReturnValue(DistantHorizonsIntegration.jitterProjection(cir.getReturnValue(),new Matrix4f()));
    }
    @WrapOperation(method="<init>",at=@At(value="INVOKE",target="Ljava/lang/reflect/Constructor;newInstance([Ljava/lang/Object;)Ljava/lang/Object;"))
    private Object rfc$afterDhInitialization(Constructor<?> constructor,Object[] args,Operation<Object> original) {
        if(!DistantHorizonsIntegration.ready()) {
            DistantHorizonsIntegration.defer();return DHCompatInternal.SHADERLESS;
        }
        return original.call(constructor,args);
    }
}
