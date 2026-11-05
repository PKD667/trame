#!/usr/bin/env python3
"""
SIMD benchmark processing: reads locked run parquet data, produces TSVs and figures.
Reads $NIXSCI_VIEW with compacted data from zen1 and avx512 experiments.
Outputs:
- TSVs with median/min/max over 3 replicates per machine
- PDF figures (speedup and throughput)
- LaTeX source for combined sheet
"""

import os
import json
from pathlib import Path
from collections import defaultdict

try:
    import pandas as pd
    HAS_PANDAS = True
except ImportError:
    print("Error: pandas required", flush=True)
    HAS_PANDAS = False
    exit(1)

try:
    import matplotlib.pyplot as plt
    import numpy as np
    HAS_MATPLOTLIB = True
except ImportError:
    print("Warning: matplotlib not available, skipping figures", flush=True)
    HAS_MATPLOTLIB = False

view = Path(os.environ['NIXSCI_VIEW'])
out = Path(os.environ['NIXSCI_OUT'])

# Color palette from aufbau.sty
COLORS = {
    'blue': '#2A78D6',      # lanes
    'purple': '#3730A3',    # scalar / accent
    'ink': '#52514E',       # text, axes
    'grid': '#D8D7D2',      # light hairlines
    'red': '#E34948',       # reference line
}

# Map experiment aliases (as they appear in the view) to output TSV files
experiments = {
    'zen1': 'simd_bench_zen1.tsv',
    'avx512': 'simd_bench_avx512.tsv',
}

combined_rows = []  # For combined file

print("Processing SIMD benchmark data...", flush=True)

for exp_name, tsv_name in experiments.items():
    exp_path = view / exp_name
    if not exp_path.exists():
        print(f"Warning: {exp_path} not found", flush=True)
        continue
    
    print(f"Processing {exp_name} from {exp_path}", flush=True)
    
    output_rows = []
    
    # Try to read parquet files
    bench_dir = exp_path / 'bench'
    if bench_dir.exists():
        parquet_files = sorted(bench_dir.glob('*.parquet'))
        print(f"  Found {len(parquet_files)} parquet files in bench/", flush=True)
        
        if parquet_files:
            try:
                # Read all parquet files
                dfs = [pd.read_parquet(f) for f in parquet_files]
                df = pd.concat(dfs, ignore_index=True)
                
                print(f"  Read {len(df)} rows from parquet (raw)", flush=True)
                
                # Aggregate across replicates: for each unique key, compute median of medians
                # and overall min/max
                agg_dict = {
                    'median_mitems_s': 'median',
                    'min_mitems_s': 'min',
                    'max_mitems_s': 'max',
                    'eq': 'first'
                }
                
                grouped = df.groupby(['machine', 'table', 'case', 'dist', 'slots', 'variant', 'items'], 
                                     as_index=False).agg(agg_dict)
                
                print(f"  Aggregated to {len(grouped)} rows", flush=True)
                
                # Convert to output rows
                for idx, row in grouped.iterrows():
                    output_rows.append({
                        'machine': row['machine'],
                        'table': row['table'],
                        'case': row['case'],
                        'dist': row['dist'],
                        'slots': int(row['slots']),
                        'variant': row['variant'],
                        'items': int(row['items']),
                        'median_Mitems/s': float(row['median_mitems_s']),
                        'min_Mitems/s': float(row['min_mitems_s']),
                        'max_Mitems/s': float(row['max_mitems_s']),
                        'eq': row['eq']
                    })
            except Exception as e:
                print(f"  Error reading parquet: {e}", flush=True)
                import traceback
                traceback.print_exc()
    
    # Write TSV for this experiment
    if output_rows:
        out_file = out / tsv_name
        with open(out_file, 'w') as f:
            headers = ['machine', 'table', 'case', 'dist', 'slots', 'variant', 'items',
                      'median_Mitems/s', 'min_Mitems/s', 'max_Mitems/s', 'eq']
            f.write('\t'.join(headers) + '\n')
            
            for row in sorted(output_rows, 
                            key=lambda r: (r['machine'], r['table'], r['case'], 
                                         r['dist'], r['slots'], r['variant'], r['items'])):
                values = [str(row[h]) for h in headers]
                f.write('\t'.join(values) + '\n')
        
        print(f"  Wrote {len(output_rows)} rows to {out_file}", flush=True)
        combined_rows.extend(output_rows)
    else:
        print(f"  No data found for {exp_name}", flush=True)

# Also write a combined TSV
if combined_rows:
    combined_file = out / 'simd_bench_combined.tsv'
    with open(combined_file, 'w') as f:
        headers = ['machine', 'table', 'case', 'dist', 'slots', 'variant', 'items',
                  'median_Mitems/s', 'min_Mitems/s', 'max_Mitems/s', 'eq']
        f.write('\t'.join(headers) + '\n')
        
        for row in sorted(combined_rows, 
                        key=lambda r: (r['machine'], r['table'], r['case'], 
                                     r['dist'], r['slots'], r['variant'], r['items'])):
            values = [str(row[h]) for h in headers]
            f.write('\t'.join(values) + '\n')
    
    print(f"Wrote combined {len(combined_rows)} rows to {combined_file}", flush=True)
    
    # Generate figures if matplotlib is available
    if HAS_MATPLOTLIB:
        print("Generating figures...", flush=True)
        
        df_all = pd.read_csv(combined_file, sep='\t')
        
        # Ensure numeric columns
        df_all['median_Mitems/s'] = pd.to_numeric(df_all['median_Mitems/s'])
        df_all['min_Mitems/s'] = pd.to_numeric(df_all['min_Mitems/s'])
        df_all['max_Mitems/s'] = pd.to_numeric(df_all['max_Mitems/s'])
        df_all['items'] = pd.to_numeric(df_all['items'])
        df_all['slots'] = pd.to_numeric(df_all['slots'])
        
        # Setup matplotlib
        plt.rcParams['font.size'] = 10
        plt.rcParams['font.family'] = 'sans-serif'
        plt.rcParams['axes.edgecolor'] = COLORS['ink']
        plt.rcParams['axes.labelcolor'] = COLORS['ink']
        plt.rcParams['xtick.color'] = COLORS['ink']
        plt.rcParams['ytick.color'] = COLORS['ink']
        plt.rcParams['text.color'] = COLORS['ink']
        plt.rcParams['figure.facecolor'] = 'white'
        plt.rcParams['axes.facecolor'] = 'white'
        plt.rcParams['axes.grid'] = True
        plt.rcParams['axes.axisbelow'] = True
        plt.rcParams['grid.color'] = COLORS['grid']
        plt.rcParams['grid.linewidth'] = 0.5
        plt.rcParams['grid.linestyle'] = '-'
        
        # Machines present in the data
        machines = sorted(df_all['machine'].unique())
        print(f"Found machines: {machines}", flush=True)
        
        # ========== Figure 1: Speedup at 1e6 items ==========
        try:
            print("Generating speedup figure...", flush=True)
            
            fig1, axes = plt.subplots(1, len(machines), figsize=(4*len(machines), 4), sharex=False, sharey=True)
            if len(machines) == 1:
                axes = [axes]
            
            # Filter for 1e6 items cases
            speedup_cases = ['axpy', 'axpy_sel', 'poly']
            speedup_dists = ['ident', 'perm']
            
            for ax_idx, machine in enumerate(machines):
                ax = axes[ax_idx]
                
                # Filter data for this machine and 1M items
                mdata = df_all[(df_all['machine'] == machine) & (df_all['items'] == 1e6) & 
                              (df_all['table'] == 'bench') & (df_all['case'].isin(speedup_cases)) &
                              (df_all['dist'].isin(speedup_dists))]
                
                # Prepare x positions and labels
                cases_dists = [(c, d) for c in speedup_cases for d in speedup_dists]
                x_pos = np.arange(len(cases_dists))
                
                # Determine the variant to use (prefer avx2 over avx2-reg/avx2-window)
                available_variants = mdata['variant'].unique()
                # Get base variants (avx2, avx512, etc) not the register variants
                base_variants = [v for v in available_variants if not ('reg' in v or 'window' in v or 'scalar' in v)]
                if not base_variants:
                    base_variants = [v for v in available_variants if 'scalar' not in v]
                
                variant_to_use = base_variants[0] if base_variants else 'avx2'
                
                # Plot bars: lanes only (no scalar)
                lanes_vals = []
                lanes_err_min = []
                lanes_err_max = []
                
                for case, dist in cases_dists:
                    # Find lanes variant
                    lanes_row = mdata[(mdata['case'] == case) & (mdata['dist'] == dist) & 
                                     (mdata['variant'] == variant_to_use)]
                    if len(lanes_row) > 0:
                        v = lanes_row.iloc[0]
                        # Get scalar throughput for speedup calculation
                        scalar_row = mdata[(mdata['case'] == case) & (mdata['dist'] == dist) & 
                                          (mdata['variant'] == 'scalar')]
                        if len(scalar_row) > 0:
                            scalar_val = scalar_row.iloc[0]['median_Mitems/s']
                            speedup_val = v['median_Mitems/s'] / scalar_val if scalar_val > 0 else 0
                            lanes_vals.append(speedup_val)
                            lanes_err_min.append((v['median_Mitems/s'] - v['min_Mitems/s']) / scalar_val if scalar_val > 0 else 0)
                            lanes_err_max.append((v['max_Mitems/s'] - v['median_Mitems/s']) / scalar_val if scalar_val > 0 else 0)
                        else:
                            lanes_vals.append(0)
                            lanes_err_min.append(0)
                            lanes_err_max.append(0)
                    else:
                        lanes_vals.append(0)
                        lanes_err_min.append(0)
                        lanes_err_max.append(0)
                
                lanes_vals = np.array(lanes_vals)
                lanes_err_min = np.array(lanes_err_min)
                lanes_err_max = np.array(lanes_err_max)
                
                # Plot bars (blue only, no scalar bars)
                ax.bar(x_pos, lanes_vals, width=0.6, color=COLORS['blue'], alpha=0.8, edgecolor='none')
                
                # Add error bars
                ax.errorbar(x_pos, lanes_vals, 
                           yerr=[lanes_err_min, lanes_err_max],
                           fmt='none', ecolor=COLORS['blue'], capsize=3, linewidth=1)
                
                # Add value labels above bars
                for i, (val, err_max) in enumerate(zip(lanes_vals, lanes_err_max)):
                    if val > 0:
                        label_y = val + err_max + 0.15
                        ax.text(i, label_y, f'{val:.1f}×', ha='center', va='bottom', 
                               fontsize=8, color=COLORS['ink'])
                
                # Reference line at 1.0 (ink color, thin dashed)
                ax.axhline(y=1.0, color=COLORS['ink'], linestyle='--', linewidth=0.8)
                
                # Labels and formatting
                ax.set_xticks(x_pos)
                ax.set_xticklabels([f'{c}\n{d}' for c, d in cases_dists], fontsize=8, rotation=0)
                ax.set_ylabel('Speedup', fontsize=10)
                
                # Build panel title with machine name and variant
                machine_short = machine.split('.')[0]
                ax.set_title(f'{machine_short} · {variant_to_use}', fontsize=10)
                
                max_speedup = max(lanes_vals) if max(lanes_vals) > 0 else 2
                ax.set_ylim([0, max_speedup * 1.2])
                ax.grid(axis='y', alpha=0.3)
            
            plt.tight_layout()
            speedup_pdf = out / 'simd_speedup.pdf'
            plt.savefig(speedup_pdf, bbox_inches='tight', dpi=150)
            print(f"Saved speedup figure to {speedup_pdf}", flush=True)
            plt.close()
        except Exception as e:
            print(f"Error generating speedup figure: {e}", flush=True)
            import traceback
            traceback.print_exc()
        
        # ========== Figure 2: Throughput vs Item Count ==========
        try:
            print("Generating throughput figure...", flush=True)
            
            fig2, axes = plt.subplots(1, len(machines), figsize=(4*len(machines), 4), sharex=False, sharey=False)
            if len(machines) == 1:
                axes = [axes]
            
            for ax_idx, machine in enumerate(machines):
                ax = axes[ax_idx]
                
                # Filter: axpy ident from bench table only
                axpy_data = df_all[(df_all['machine'] == machine) & (df_all['table'] == 'bench') &
                                  (df_all['case'] == 'axpy') & (df_all['dist'] == 'ident')]
                
                # Plot each (case, variant) combination separately with its own min-max band
                # Group by variant
                for variant in ['scalar', 'avx2']:
                    variant_data = axpy_data[axpy_data['variant'] == variant]
                    if len(variant_data) > 0:
                        variant_data = variant_data.sort_values('items')
                        if variant == 'scalar':
                            ax.plot(variant_data['items'], variant_data['median_Mitems/s'],
                                   marker='o', label='axpy / scalar', color=COLORS['purple'], linewidth=2)
                            ax.fill_between(variant_data['items'], 
                                           variant_data['min_Mitems/s'], variant_data['max_Mitems/s'],
                                           alpha=0.2, color=COLORS['purple'])
                        else:
                            ax.plot(variant_data['items'], variant_data['median_Mitems/s'],
                                   marker='s', label='axpy / avx2', color=COLORS['blue'], linewidth=2)
                            ax.fill_between(variant_data['items'], 
                                           variant_data['min_Mitems/s'], variant_data['max_Mitems/s'],
                                           alpha=0.2, color=COLORS['blue'])
                
                # sum32 uniform 8: include both bench and bench_knee tables
                sum32_data = df_all[(df_all['machine'] == machine) & 
                                   (df_all['case'] == 'sum32') & (df_all['dist'] == 'uniform') & (df_all['slots'] == 8)]
                
                # Plot each variant separately
                # Get all unique variants and separate into scalar and lane variants
                sum32_variants = sum32_data['variant'].unique()
                scalar_sum32_data = sum32_data[sum32_data['variant'] == 'scalar']
                lane_sum32_data = sum32_data[sum32_data['variant'] != 'scalar']
                
                # Plot scalar
                if len(scalar_sum32_data) > 0:
                    scalar_sum32_data = scalar_sum32_data.sort_values('items')
                    ax.plot(scalar_sum32_data['items'], scalar_sum32_data['median_Mitems/s'],
                           marker='o', linestyle='--', label='sum32 / scalar', 
                           color=COLORS['purple'], linewidth=2, alpha=0.6)
                    ax.fill_between(scalar_sum32_data['items'], 
                                   scalar_sum32_data['min_Mitems/s'], scalar_sum32_data['max_Mitems/s'],
                                   alpha=0.2, color=COLORS['purple'])
                
                # Plot first lane variant found
                if len(lane_sum32_data) > 0:
                    lane_variant = lane_sum32_data['variant'].iloc[0]
                    lane_sum32_data = lane_sum32_data[lane_sum32_data['variant'] == lane_variant].sort_values('items')
                    ax.plot(lane_sum32_data['items'], lane_sum32_data['median_Mitems/s'],
                           marker='s', linestyle='--', label=f'sum32 / {lane_variant}', 
                           color=COLORS['blue'], linewidth=2, alpha=0.6)
                    ax.fill_between(lane_sum32_data['items'], 
                                   lane_sum32_data['min_Mitems/s'], lane_sum32_data['max_Mitems/s'],
                                   alpha=0.2, color=COLORS['blue'])
                
                # Set log scale
                ax.set_xscale('log')
                ax.set_yscale('log')
                
                # Use plain tick labels for log scale (not 10^x notation)
                # Set custom yticks with plain labels
                yticks = [300, 1000, 3000, 10000]
                ax.set_yticks(yticks)
                ax.set_yticklabels([str(y) for y in yticks])
                
                ax.set_xlabel('Items', fontsize=10)
                ax.set_ylabel('Throughput (Mitems/s)', fontsize=10)
                
                machine_short = machine.split('.')[0]
                ax.set_title(f'{machine_short}', fontsize=10)
                ax.grid(True, which='both', alpha=0.3)
                ax.legend(fontsize=8, loc='best')
            
            plt.tight_layout()
            throughput_pdf = out / 'simd_throughput.pdf'
            plt.savefig(throughput_pdf, bbox_inches='tight', dpi=150)
            print(f"Saved throughput figure to {throughput_pdf}", flush=True)
            plt.close()
        except Exception as e:
            print(f"Error generating throughput figure: {e}", flush=True)
            import traceback
            traceback.print_exc()
else:
    print("No combined data, skipping figures", flush=True)

print("Processing complete", flush=True)
