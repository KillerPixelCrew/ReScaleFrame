package org.kpc.rescaleframecraft.mixin.flywheel;

import net.irisshaders.iris.shaderpack.option.values.OptionValues;
import org.kpc.rescaleframecraft.render.IrisIntegration;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyVariable;

@Pseudo
@Mixin(targets="net.irisshaders.iris.shaderpack.option.OptionAnnotatedSource",remap=false)
abstract class IrisOptionsMixin {
    @ModifyVariable(method="asTransform",at=@At("HEAD"),argsOnly=true)
    private OptionValues rfc$temporalOptions(OptionValues options) {return IrisIntegration.runtimeOptions(options);}
}
