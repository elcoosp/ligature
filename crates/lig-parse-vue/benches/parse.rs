//! Parse/lowering throughput bench for the Vue frontend (§14.2 — benched
//! from day one): lower the counter SFC, the same workload the Solid
//! frontend's `parse` bench drives.

use criterion::{Criterion, criterion_group, criterion_main};

const COUNTER_VUE: &str = r#"<script setup lang="ts">
import { ref, computed } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
</script>

<template>
  <div class="counter p-4" :style="{ display: 'flex', padding: '16px' }">
    <button @click="count = count + 1">Add</button>
    <span>{{ count }}</span>
    <p v-if="doubled > 10" class="hot">Big: {{ doubled }}</p>
  </div>
</template>
"#;

fn lower_counter() {
    lig_parse_vue::lower_module(COUNTER_VUE, "counter.vue").expect("counter lowers");
}

fn bench_lower(c: &mut Criterion) {
    c.bench_function("vue/counter", |b| b.iter(lower_counter));
}

criterion_group!(benches, bench_lower);
criterion_main!(benches);
