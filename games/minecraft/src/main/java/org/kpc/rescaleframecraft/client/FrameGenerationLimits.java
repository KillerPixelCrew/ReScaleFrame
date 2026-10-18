package org.kpc.rescaleframecraft.client;

import net.caffeinemc.mods.sodium.api.config.option.SteppedValidator;
import java.util.function.Supplier;
import java.util.Arrays;

/** Session capabilities reported by the selected native FG provider, independent of the SR choice. */
public final class FrameGenerationLimits {
    public static final FrameGenerationLimits INSTANCE=new FrameGenerationLimits();
    private final int[] maximum=new int[8];
    private int automatic;
    public void reset() {Arrays.fill(maximum,0);automatic=0;}
    public boolean record(int requested,int effective,int generated,boolean supported) {
        if(effective<=0||effective>=maximum.length||effective==7||!supported||generated<1)return false;
        int value=Math.clamp(generated,1,15);
        boolean changed=maximum[effective]!=value;maximum[effective]=value;
        if(requested==7&&automatic!=effective) {automatic=effective;changed=true;}
        return changed;
    }
    public int maximum(TaaSettings.FrameGeneration selected) {
        int backend=selected==TaaSettings.FrameGeneration.AUTO?automatic:selected.id;
        return backend==0?1:Math.max(1,maximum[backend]);
    }
    public int clamp(TaaSettings.FrameGeneration selected,int generated) {return Math.clamp(generated,1,maximum(selected));}
    public SteppedValidator range(TaaSettings.FrameGeneration selected) {
        return new SteppedValidator() {
            @Override public int min() {return 1;}
            @Override public int max() {return maximum(selected);}
            @Override public int step() {return 1;}
            @Override public Integer getValidatedValue(Integer value,Supplier<Integer> fallback) {return clamp(selected,value);}
        };
    }
}
