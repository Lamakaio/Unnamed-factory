//helper
fn clamp(val, l, h) {
    min(max(val, l), h)
}

// Compute growth
data.resource.dfood =   data.job.collecter.population * 
                        data.job.collecter.productivity * 
                        (1.0 + data.building.farm_area) //some basic food prod, greatly increased by farm area
                    -   data.aggregates.population * data.aggregates.avg_happiness * 0.2;

data.resource.dmaterial =   data.job.collecter.population * 
                            data.job.collecter.productivity * 
                            (1.0 + data.building.mine_area); //some basic mat prod, inc by mines


data.stat.dfame = data.aggregates.avg_happiness;

// Resource actualization 
data.resource.food *= data.resource.food_spoilage;
data.resource.food += data.resource.dfood;
data.resource.material += data.resource.dmaterial;
data.stat.fame += data.stat.dfame;

// Food shortage
let food_shortage = if (data.resource.dfood < 0.0) { clamp(- data.resource.food / data.resource.dfood * 0.1, 0., 1.)} 
                    else {1.0};

let emergencypop = data.stat.idleness * min((1. - food_shortage), 0.5);
data.stat.idleness -= emergencypop;
data.job.collecter.population += emergencypop;

// Compute stats and temp resources
let hab_ratio = min(1., max(0., 
                    (2*data.building.habitations - data.aggregates.population ) / data.aggregates.population));

data.stat.migration = ((data.aggregates.avg_happiness*0.1 + data.aggregates.avg_demand * 0.3) 
                        * (data.aggregates.population - data.stat.idleness)) * food_shortage * 0.05 * hab_ratio ;
data.stat.natality  = data.aggregates.avg_happiness * data.aggregates.population * max(food_shortage, 0.5) * 0.1 * hab_ratio;
data.stat.science = data.job.researcher.population * 
                    data.job.researcher.productivity * 
                    (1.0 + data.building.work_area); //some basic science, inc by workshops

let n = data.job.len();
// Compute generic job stats
let generic_productivity = 0.1 + 2. * (data.job.crafter.population * 
                                data.job.crafter.productivity * 
                                (1.0 + data.building.work_area)
                                +
                                data.job.teacher.population * data.job.teacher.productivity
                            ) / data.aggregates.population
                            + data.aggregates.avg_happiness * 0.1;

// Compute generic job stats
let generic_happiness = data.job.artist.population / data.aggregates.population * 2.;

for k in data.job.keys() {
    let demand_ratio = (data.job[k].demand)
                    / (n * data.aggregates.avg_demand);
    //growth as dictated by demand
    let ideal_growth = (data.stat.migration + data.stat.natality) * demand_ratio;

    //if ppl are unhappy, they're not gonna work
    let happiness_factor = data.job[k].happiness / data.aims.happiness;
    if data.stat.idleness > (happiness_factor - 1.) * ideal_growth {
        data.stat.idleness -= (happiness_factor - 1.) * ideal_growth;
    }

    data.job[k].dpopulation = happiness_factor * ideal_growth;
    data.job[k].productivity = data.job[k].specific_prod + generic_productivity;
    data.job[k].happiness = generic_happiness - (data.job[k].commute - 0.5);
    data.job[k].population *= data.stat.death_rate;
    data.job[k].population += data.job[k].dpopulation;
}

data.stat.idleness *= data.stat.death_rate; //idle ppl die too
    

data.job.teacher.demand = data.aggregates.population / max(data.job.teacher.population * 10., 1.);
data.job.researcher.demand = data.stat.idleness / data.aggregates.population;
data.job.artist.demand = (data.stat.idleness / data.aggregates.population) * 10.
                + (data.aims.happiness / data.aggregates.avg_happiness);
data.job.collecter.demand = data.aggregates.population / max(data.resource.food, 1.0);
data.job.crafter.demand = (data.aims.productivity / data.aggregates.avg_productivity) * 2.
                        - (data.aims.happiness / data.aggregates.avg_happiness) / 2.;
// compute aggregates 

let acc_pop = 0.0;
let acc_prod = 0.0;
let acc_happ = 0.0;
let acc_comm = 0.0;
let acc_demand = 0.0;

for k in data.job.keys() {
    acc_pop +=  data.job[k].population;
    acc_prod += data.job[k].productivity;
    acc_happ += data.job[k].happiness;
    acc_comm += data.job[k].commute;
    acc_demand += data.job[k].demand;
}

data.aggregates.population = acc_pop * (1.0 + data.stat.idleness/acc_pop);
data.aggregates.avg_productivity = acc_prod / n;
data.aggregates.avg_happiness = acc_happ / n;
data.aggregates.avg_commute = acc_comm / n;
data.aggregates.avg_demand = acc_demand / n;