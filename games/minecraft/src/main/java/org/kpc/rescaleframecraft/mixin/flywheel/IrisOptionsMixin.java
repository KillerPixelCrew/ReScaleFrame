package org.kpc.rescaleframecraft.mixin.flywheel;

import net.irisshaders.iris.shaderpack.option.values.OptionValues;
import net.irisshaders.iris.shaderpack.transform.line.LineTransform;
import com.google.common.collect.ImmutableList;
import org.kpc.rescaleframecraft.render.IrisIntegration;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Pseudo
@Mixin(targets="net.irisshaders.iris.shaderpack.option.OptionAnnotatedSource",remap=false)
abstract class IrisOptionsMixin {
    @Shadow @Final private ImmutableList<String> lines;
    @ModifyVariable(method="asTransform",at=@At("HEAD"),argsOnly=true)
    private OptionValues rfc$temporalOptions(OptionValues options) {return IrisIntegration.runtimeOptions(options);}
    @Inject(method="asTransform",at=@At("RETURN"),cancellable=true)
    private void rfc$temporalSampling(OptionValues options,CallbackInfoReturnable<LineTransform> cir) {
        cir.setReturnValue(IrisIntegration.runtimeSampling(cir.getReturnValue(),lines,options));
    }
}
