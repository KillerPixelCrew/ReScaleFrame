package org.kpc.rescaleframecraft.render;

import java.io.IOException;
import java.util.Map;
import java.util.List;
import java.util.HashSet;
import java.util.WeakHashMap;
import net.irisshaders.iris.Iris;
import net.irisshaders.iris.shaderpack.option.values.OptionValues;
import net.irisshaders.iris.shaderpack.transform.line.LineTransform;
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
    public static OptionValues runtimeOptions(OptionValues original) {return runtimeOptions(original,ownsTemporal());}
    static synchronized OptionValues runtimeOptions(OptionValues original,boolean owner) {
        if(!owner)return original;
        return effective.computeIfAbsent(original,options->{
            var result=options.mutableCopy();
            // Complementary exposes TAA_DEFINE; other named boolean options are changed only if declared.
            if(options.getOptionSet().getStringOptions().containsKey("TAA_DEFINE"))result.addAll(Map.of("TAA_DEFINE","0"));
            for(String name:new String[]{"TAA","TAA_ENABLED","TEMPORAL_AA","ENABLE_TAA"})
                if(options.getOptionSet().getBooleanOptions().containsKey(name))result.addAll(Map.of(name,"false"));
            return result;
        });
    }
    public static LineTransform runtimeSampling(LineTransform original,List<String> lines,OptionValues options) {
        return runtimeSampling(original,lines,ownsTemporal()&&options.getOptionSet().getStringOptions().containsKey("TAA_DEFINE"));
    }
    static LineTransform runtimeSampling(LineTransform original,List<String> lines,boolean owner) {
        if(!owner)return original;
        var samplingConditions=new HashSet<Integer>();
        for(int i=0;i+1<lines.size();i++) {
            String condition=lines.get(i).replaceAll("\\s+","");
            if(!condition.equals("#ifdefTAA")&&!condition.equals("#ifdefinedTAA||definedPBR_REFLECTIONS"))continue;
            String sample=lines.get(i+1).replaceAll("\\s+","");
            // Complementary gates stochastic atmospheric sampling on its TAA
            // switch. Preserve that producer for the external temporal resolver;
            // unrelated TAA branches (projection jitter and resolve) stay off.
            if(sample.equals("dither=fract(dither+goldenRatio*mod(float(frameCounter),3600.0));")||
                sample.equals("returnfract(n+goldenRatio*mod(float(frameCounter),3600.0));"))samplingConditions.add(i);
        }
        if(samplingConditions.isEmpty())return original;
        ReScaleFrameCraft.LOGGER.debug("Preserving {} Complementary temporal sampling branches",samplingConditions.size());
        return (index,line)->samplingConditions.contains(index)?"#if 1 // ReScaleFrame temporal sampling":original.transform(index,line);
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
