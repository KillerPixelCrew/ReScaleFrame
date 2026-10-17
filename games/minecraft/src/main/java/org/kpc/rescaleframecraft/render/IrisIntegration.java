package org.kpc.rescaleframecraft.render;

import java.io.IOException;
import java.util.Map;
import java.util.WeakHashMap;
import net.irisshaders.iris.Iris;
import net.irisshaders.iris.shaderpack.option.values.OptionValues;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.client.TaaSettings;
import org.kpc.rescaleframecraft.render.flywheel.IrisState;

/** A single temporal owner; pack preferences stay intact while effective shader defines are overridden. */
public final class IrisIntegration {
    private static final Map<OptionValues,OptionValues> effective=new WeakHashMap<>();
    private static Boolean previousOwner;
    private IrisIntegration() {}
    private static boolean ownsTemporal() {
        var settings=TaaSettings.current;
        return settings.enabled||settings.upscaler!=TaaSettings.Upscaler.OFF||settings.frameGeneration!=TaaSettings.FrameGeneration.OFF;
    }
    public static synchronized OptionValues runtimeOptions(OptionValues original) {
        if(!ownsTemporal())return original;
        return effective.computeIfAbsent(original,options->{
            var result=options.mutableCopy();
            // Complementary exposes TAA_DEFINE; other named boolean options are changed only if declared.
            if(options.getOptionSet().getStringOptions().containsKey("TAA_DEFINE"))result.addAll(Map.of("TAA_DEFINE","0"));
            for(String name:new String[]{"TAA","TAA_ENABLED","TEMPORAL_AA","ENABLE_TAA"})
                if(options.getOptionSet().getBooleanOptions().containsKey(name))result.addAll(Map.of(name,"false"));
            return result;
        });
    }
    public static void frame() {
        boolean owner=ownsTemporal();
        if(previousOwner==null) {previousOwner=owner;return;}
        if(previousOwner==owner)return;
        previousOwner=owner;
        synchronized(IrisIntegration.class) {effective.clear();}
        if(!IrisState.packActive())return;
        try {Iris.reload();TemporalRenderer.reset();}
        catch(IOException failure) {ReScaleFrameCraft.LOGGER.error("Iris reload after temporal-owner change failed",failure);}
    }
}
