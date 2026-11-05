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
                
                # Plot bars: scalar and lanes (avx2 or avx512)
                scalar_vals = []
                lanes_vals = []
                scalar_err_min = []
                scalar_err_max = []
                lanes_err_min = []
                lanes_err_max = []
                
                for case, dist in cases_dists:
                    # Scalar
                    scalar_row = mdata[(mdata['case'] == case) & (mdata['dist'] == dist) & 
                                      (mdata['variant'] == 'scalar')]
                    if len(scalar_row) > 0:
                        scalar_vals.append(scalar_row['median_Mitems/s'].values[0])
                        scalar_err_min.append(scalar_row['median_Mitems/s'].values[0] - scalar_row['min_Mitems/s'].values[0])
                        scalar_err_max.append(scalar_row['max_Mitems/s'].values[0] - scalar_row['median_Mitems/s'].values[0])
                    else:
                        scalar_vals.append(0)
                        scalar_err_min.append(0)
                        scalar_err_max.append(0)
                    
                    # Find lanes variant
                    lanes_variants = mdata[(mdata['case'] == case) & (mdata['dist'] == dist) & 
                                          (~mdata['variant'].str.contains('scalar'))]
                    if len(lanes_variants) > 0:
                        v = lanes_variants.iloc[0]
                        lanes_vals.append(v['median_Mitems/s'])
                        lanes_err_min.append(v['median_Mitems/s'] - v['min_Mitems/s'])
                        lanes_err_max.append(v['max_Mitems/s'] - v['median_Mitems/s'])
                    else:
                        lanes_vals.append(0)
                        lanes_err_min.append(0)
                        lanes_err_max.append(0)
                
                # Plot with intervals
                width = 0.35
                x_scalar = x_pos - width/2
                x_lanes = x_pos + width/2
                
                # Convert throughput to speedup
                scalar_vals = np.array(scalar_vals)
                lanes_vals = np.array(lanes_vals)
                speedup = np.where(scalar_vals > 0, lanes_vals / scalar_vals, 0)
                speedup_err_min = np.where(scalar_vals > 0, 
                                          np.array(lanes_err_min) / scalar_vals, 0)
                speedup_err_max = np.where(scalar_vals > 0, 
                                          np.array(lanes_err_max) / scalar_vals, 0)
                
                # Plot
                ax.bar(x_scalar, [1]*len(x_pos), width=width, 
                      label='scalar', color='white', edgecolor=COLORS['purple'], linewidth=1.5)
                ax.bar(x_lanes, speedup, width=width, 
                      label='lanes', color=COLORS['blue'], alpha=0.7)
                
                # Add interval bars
                ax.errorbar(x_lanes, speedup, 
                           yerr=[speedup_err_min, speedup_err_max],
                           fmt='none', ecolor=COLORS['blue'], capsize=3, linewidth=1)
                
                # Reference line at 1.0
                ax.axhline(y=1.0, color=COLORS['red'], linestyle='-', linewidth=1, alpha=0.5)
                
                # Labels and formatting
                ax.set_xticks(x_pos)
                ax.set_xticklabels([f'{c}\n{d}' for c, d in cases_dists], fontsize=8, rotation=0)
                ax.set_ylabel('Speedup', fontsize=10)
                ax.set_title(f'{machine}', fontsize=10)
                max_speedup = max(speedup) if max(speedup) > 0 else 2
                ax.set_ylim([0, max_speedup * 1.2])
                ax.grid(axis='y', alpha=0.3)
            
            plt.suptitle('SIMD Speedup (lanes/scalar) at 1M items', fontsize=12, y=0.98)
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
                
                # Filter: axpy ident
                axpy_data = df_all[(df_all['machine'] == machine) & (df_all['table'] == 'bench') &
                                  (df_all['case'] == 'axpy') & (df_all['dist'] == 'ident')]
                
                # Separate by variant
                scalar_axpy = axpy_data[axpy_data['variant'] == 'scalar']
                if len(scalar_axpy) > 0:
                    scalar_axpy = scalar_axpy.sort_values('items')
                    ax.plot(scalar_axpy['items'], scalar_axpy['median_Mitems/s'],
                           marker='o', label='scalar (axpy)', color=COLORS['purple'], linewidth=2)
                    ax.fill_between(scalar_axpy['items'], 
                                   scalar_axpy['min_Mitems/s'], scalar_axpy['max_Mitems/s'],
                                   alpha=0.2, color=COLORS['purple'])
                
                # Lane variants
                lanes_axpy = axpy_data[~axpy_data['variant'].str.contains('scalar')]
                if len(lanes_axpy) > 0:
                    lanes_axpy = lanes_axpy.sort_values('items')
                    ax.plot(lanes_axpy['items'], lanes_axpy['median_Mitems/s'],
                           marker='s', label='lanes (axpy)', color=COLORS['blue'], linewidth=2)
                    ax.fill_between(lanes_axpy['items'], 
                                   lanes_axpy['min_Mitems/s'], lanes_axpy['max_Mitems/s'],
                                   alpha=0.2, color=COLORS['blue'])
                
                # sum32 uniform 8 keys
                sum32_data = df_all[(df_all['machine'] == machine) & (df_all['table'] == 'bench') &
                                   (df_all['case'] == 'sum32') & (df_all['dist'] == 'uniform') & (df_all['slots'] == 8)]
                
                scalar_sum32 = sum32_data[sum32_data['variant'] == 'scalar']
                if len(scalar_sum32) > 0:
                    scalar_sum32 = scalar_sum32.sort_values('items')
                    ax.plot(scalar_sum32['items'], scalar_sum32['median_Mitems/s'],
                           marker='o', linestyle='--', label='scalar (sum32)', 
                           color=COLORS['purple'], linewidth=2, alpha=0.6)
                
                lanes_sum32 = sum32_data[~sum32_data['variant'].str.contains('scalar')]
                if len(lanes_sum32) > 0:
                    lanes_sum32 = lanes_sum32.sort_values('items')
                    ax.plot(lanes_sum32['items'], lanes_sum32['median_Mitems/s'],
                           marker='s', linestyle='--', label='lanes (sum32)', 
                           color=COLORS['blue'], linewidth=2, alpha=0.6)
                
                # Set log scale
                ax.set_xscale('log')
                ax.set_yscale('log')
                ax.set_xlabel('Items', fontsize=10)
                ax.set_ylabel('Throughput (Mitems/s)', fontsize=10)
                ax.set_title(f'{machine}', fontsize=10)
                ax.grid(True, which='both', alpha=0.3)
                ax.legend(fontsize=8, loc='best')
            
            plt.suptitle('Throughput vs Item Count (log scale)', fontsize=12, y=0.98)
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
