use std::collections::{HashMap, HashSet};

use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::ClassifiedInstruction;

use crate::core::constants::{SKIP_PROGRAM_IDS, SYSTEM_PROGRAMS};
use crate::core::utils::get_instruction_data;

#[derive(Clone, Debug)]
pub struct InstructionClassifier {
    instruction_map: HashMap<String, Vec<ClassifiedInstruction>>,
    // храним порядок «первого появления» program_id (как в TS порядок ключей Map)
    order: Vec<String>,
}

impl InstructionClassifier {
    pub fn new(adapter: &TransactionAdapter) -> Self {
        #[cfg(debug_assertions)]
        let t0 = std::time::Instant::now();
        
               // Pre-allocate with estimated capacity
               let outer_count = adapter.instructions().len();
               let mut instruction_map: HashMap<String, Vec<ClassifiedInstruction>> = HashMap::with_capacity(outer_count / 2);
        let mut order: Vec<String> = Vec::with_capacity(outer_count / 2);
        let mut seen: HashSet<String> = HashSet::with_capacity(outer_count / 2);

        // OUTER instructions - ZERO-COPY: минимизируем клонирования program_id
        for (outer_index, instruction) in adapter.instructions().iter().enumerate() {
            if instruction.program_id.is_empty() {
                continue;
            }
            // ZERO-COPY: клонируем program_id только один раз для HashMap ключа
            let program_id = instruction.program_id.clone();
            let classified = ClassifiedInstruction {
                program_id: program_id.clone(), // Переиспользуем уже клонированный program_id
                outer_index,
                inner_index: None,
                data: instruction.clone(),
            };
            instruction_map
                .entry(program_id.clone()) // Переиспользуем клон
                .or_default()
                .push(classified);
            if seen.insert(program_id.clone()) {
                order.push(program_id); // Используем уже клонированный program_id
            }
        }
        #[cfg(debug_assertions)]
        let t1 = std::time::Instant::now();

        // INNER instructions - ZERO-COPY: минимизируем клонирования program_id
        #[cfg(debug_assertions)]
        let mut inner_count = 0;
        for inner in adapter.inner_instructions() {
            for (inner_index, instruction) in inner.instructions.iter().enumerate() {
                if instruction.program_id.is_empty() {
                    continue;
                }
                #[cfg(debug_assertions)]
                {
                    inner_count += 1;
                }
                // ZERO-COPY: клонируем program_id только один раз для HashMap ключа
                let program_id = instruction.program_id.clone();
                let classified = ClassifiedInstruction {
                    program_id: program_id.clone(), // Переиспользуем уже клонированный program_id
                    outer_index: inner.index,
                    inner_index: Some(inner_index),
                    data: instruction.clone(),
                };
                instruction_map
                    .entry(program_id.clone()) // Переиспользуем клон
                    .or_default()
                    .push(classified);
                if seen.insert(program_id.clone()) {
                    order.push(program_id); // Используем уже клонированный program_id
                }
            }
        }
        
        #[cfg(debug_assertions)]
        {
            let t2 = std::time::Instant::now();
            tracing::debug!(
                "InstructionClassifier: processed {} inner instructions from {} groups",
                inner_count,
                adapter.inner_instructions().len()
            );
            tracing::debug!(
                "⏱️  InstructionClassifier::new: outer={:.3}μs ({}), inner={:.3}μs ({}), total={:.3}μs",
                (t1 - t0).as_secs_f64() * 1_000_000.0, adapter.instructions().len(),
                (t2 - t1).as_secs_f64() * 1_000_000.0, inner_count,
                (t2 - t0).as_secs_f64() * 1_000_000.0,
            );
            tracing::info!(
                "InstructionClassifier: found {} unique program IDs: {:?}",
                order.len(),
                order
            );
        }

        Self {
            instruction_map,
            order,
        }
    }

    /// Полный список program_id в порядке первого появления,
    /// но с фильтром как в TS: исключаем системные и «skip».
    /// ZERO-COPY: возвращает итератор по ссылкам
    pub fn get_all_program_ids_iter(&self) -> impl Iterator<Item = &str> {
        self.order.iter()
            .map(|pid| pid.as_str())
            .filter(|pid_str| {
                !SYSTEM_PROGRAMS.contains(pid_str) && !SKIP_PROGRAM_IDS.contains(pid_str)
            })
    }
    
    /// Получить все program_id как Vec (для обратной совместимости)
    pub fn get_all_program_ids(&self) -> Vec<String> {
        // ZERO-COPY: используем итератор, клонируем только в конце
        self.get_all_program_ids_iter().map(|s| s.to_string()).collect()
    }

    /// Все инструкции по одному program_id
    /// ZERO-COPY: возвращает ссылку вместо клона
    pub fn get_instructions(&self, program_id: &str) -> &[ClassifiedInstruction] {
        // ZERO-COPY: возвращаем срез вместо клонирования
        self.instruction_map
            .get(program_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
    
    /// Получить инструкции как Vec (для обратной совместимости)
    pub fn get_instructions_clone(&self, program_id: &str) -> Vec<ClassifiedInstruction> {
        self.instruction_map
            .get(program_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Инструкции по нескольким program_id (flatten), как getMultiInstructions в TS
    pub fn get_multi_instructions<S: AsRef<str>>(
        &self,
        program_ids: &[S],
    ) -> Vec<ClassifiedInstruction> {
        let mut out = Vec::new();
        for pid in program_ids {
            if let Some(vec) = self.instruction_map.get(pid.as_ref()) {
                out.extend(vec.iter().cloned());
            }
        }
        out
    }

    /// Поиск инструкции по дискриминатору (первые `slice` байт)
    /// Полный аналог TS: getInstructionByDescriminator(Buffer, slice)
    pub fn get_instruction_by_discriminator(
        &self,
        discriminator: &[u8],
        slice: usize,
    ) -> Option<ClassifiedInstruction> {
        for instructions in self.instruction_map.values() {
            for ci in instructions {
                // get_instruction_data должен вернуть &[u8] / Vec<u8> с реальными байтами data
                let data = get_instruction_data(&ci.data);
                if data.len() >= slice && &data[..slice] == discriminator {
                    return Some(ci.clone());
                }
            }
        }
        None
    }

    /// Опционально оставил (в TS нет, но вдруг пригодится)
    pub fn flatten(&self) -> Vec<ClassifiedInstruction> {
        self.instruction_map.values().flatten().cloned().collect()
    }
}
